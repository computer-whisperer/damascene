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

### Phase B rulings (2026-10-14)

Ruled by the user after the fix phase, each a visible default change:

1. **Zinc gains the surface ladder and stays the default.** `card` and
   `popover` move to zinc-900 over a zinc-950 `background`, which is
   shadcn's current v4 zinc dark theme. Rejected: making slate-blue
   the default (a colour change on top of a structural one).
2. **Tree and dense table rows stay 22 px.** The `SmallHitTarget` lint
   exempts a full-width row inside a vertical arrow-nav group: no
   horizontal ambiguity, hit inflation does not spill between rows,
   and Up/Down is the equivalent path. Rejected: raising rows to 24 px
   (VS Code, Finder and every tree surveyed sit at 22).
3. **Primary-button text on blue is white.** The fork's contrast
   retune had taken Radix blue dark-1 for 5.62:1; white measures
   3.26:1 and is what Radix, shadcn and VS Code all ship. `primary` is
   held to the 3:1 large-text floor; status chips keep 4.5:1.
4. **The rest of the Opus cheap wins, as stated:** dot-plus-text badges
   with the box as an explicit variant, underline tabs that hug and
   left-align (the segmented look is `toggle_group`), neutral selected
   fills everywhere, neutral disabled fills, ghost buttons inside
   toolbars plus `split_button` and `icon_button_with_caption`, a
   section primitive in core, the Lucide wholesale import, and the
   workbench crate held unpublished at 0.9.0. 0.9.0 is not released
   until the breaking set is complete.

**Landed (2026-10-14, nine commits on main):** `2b04241` zinc surface
step + white primary label; `a68c7ab` the two lint rulings (primary
pair at 3:1, full-width rows in a vertical arrow-nav group); `02e17c8`
neutral selected and disabled; `161d3ca` dot-plus-text badge with
`.boxed()`; `141d0d8` underline tabs; `da2edf6` toolbar ghost rule,
`split_button`, `icon_button_with_caption`, composite variants;
`53a78b1` Lucide 1.57.0 wholesale by generator; `6a314b9` the
`section` primitive; plus the re-rendered README hero. The workbench
crate was already `publish = false`. Still open from the review's
smaller list: `apply_metrics` idempotence on retained trees (document),
`caret_scroll_request_for` cannot see the type scale, a `data_action`
cell helper, the `.size()`-on-inert-role and `UnregisteredToken` lints,
the icon gallery fixture's long labels overflowing their tiles, and a
wasm feature gate for the 600 KB icon table if binary size bites.

**Ruled 2026-10-16, the two 0.9.0 questions left open above:**
`InteractionState::Disabled` and `Loading` are removed — the runtime
never produced them (`.disabled()` / `.loading()` bake their look at
build time) and the painter's arms for them were already gone. The
`IconName` renames to Lucide's canon are a hard break: the generated
alias constants (`IconName::AlertCircle` for `CircleAlert`, 260 of
them) are gone, so a retired variant name no longer compiles; the
string side is unchanged, `IconName::parse("alert-circle")` still
resolves through Lucide's own alias metadata and the house short forms.
Rejected: keeping the constants (a 0.9.0 that renames and keeps the old
names is not a rename, and the generator would carry a second namespace
forever).

### Blind round 1 (2026-10-16)

The measurement the decisions above kept asking for: a fresh agent, a
brief, and the library, with the output scored visually. The setup is
reusable and lives outside the repo at
`~/workspace/damascene/blind-rounds/2026-10-16-readinglist` (its own
git: the brief and path deps, the agent's `v1`, then the scoring
shots). The brief asks for a personal reading list: a master-detail
desktop tool with a status filter and per-status counts, add, edit and
delete, a 1–5 rating for finished books, Tab reaching everything and
Enter submitting the add form, and "it should look like a well-made
desktop tool". The agent (a general-purpose subagent with no other
context) builds against the local main checkout after Phase B, may not
modify the library, keeps a journal of every cargo attempt, and is
told not to look at SVG or PNG renders in the first pass, so `v1` is
the default parse rather than a screenshot-tuned result. Scoring is a
headless shot through `DAMASCENE_HEADLESS_SHOT` at 1x and 2x, three
more states rendered through `damascene_wgpu::headless` from a test,
the lint pass over four scenes, and a count of explicit style calls in
the source.

**Result.** Fourteen cargo invocations, no compile error in app code
(the two errors were test-side: `UiEvent` and `KeyPress` are
`#[non_exhaustive]`, so a synthesised key event cannot use a struct
literal), and nine lint findings the library caught before any human
looked: a `select_trigger` inside a `field` with no accessible name,
and hit-overflow plus focus-ring collisions on five ghost star buttons
at gap 0. Every piece of the brief mapped onto a catalog widget
(`page`, `toolbar`, `section`, `tabs_list_from_triggers` with count
badges, `item`/`item_group` in a `scroll`, `card` with `field`s,
`select`, `icon_button`, `badge`, `kbd`, toasts, hotkeys). No raw
color, font, radius, fill or stroke anywhere; two raw sizes (the
inspector width and the window). The 1x and 2x shots read as a
competent desktop tool: surface ladder, underline tabs, dot badges,
selected rail, white primary, destructive delete, all from defaults.

**What the round exposed.** Each is a default or a doc, listed for
ruling, none fixed yet:

1. **Ragged left edges in a stacked pane.** `column([section,
   tabs_list, scroll([item_group])])` puts the section title at 16, the
   section body's content at 24 (the `pl = SPACE_2` inset in the
   section anatomy), tab labels at 26 (trigger padding) and item tiles
   at 30. The inset under the section title is the one with no surface
   to justify it; the others are padding inside a hover/selected shape.
2. **The select popover hugs its options** while the trigger fills its
   field, so an open status menu is a third of the width of the control
   above it. shadcn sizes the content to at least the trigger width;
   `popover` has no anchor-width mode to opt into.
3. **`field(label, control)` does no label association.** A combobox
   in a field fails `NoAccessibleName`; a `text_input` passes only
   because its placeholder names it. HTML's `<label for>` is the shape
   the author expects.
4. **No filled glyphs.** Lucide is stroke-only, so a rating is five
   orange outlines against five grey ones; a lit star wants a fill.
5. **Enter in a `text_input` is undocumented at the README level** and
   has no helper. The widget captures keys, so Enter arrives as a raw
   `KeyDown`; the agent read `runtime.rs` to learn this and matched
   `NamedKey::Enter` on the two input keys by hand.
6. Two documented answers for an enum picker (README: `select`;
   `toggle.rs`: `toggle_group`); `select::apply_event` takes its parse
   closure by `String` where `tabs` and `toggle` take `&str`; the
   README's testing section shows only `synthetic_click`.

The brief itself needs one line it lacked: the lint lives in the
bundle pipeline and only runs through app-written test code, so a
brief that says "the library's lint checks" should say a lint test in
the project is expected. The agent wrote one unprompted; another may
not.

**Phase 2 (same agent, allowed to look).** The agent was then told
to use the review tooling the library documents, view renders, and
improve the look without changing features, journaling every change
with its own verdict on where the fix belongs. It committed `v2`
(`b1706a1`, clippy and lint clean, six scenes rendered through
`damascene-wgpu`). Five visual changes, by its own classification:
rating stars to the Xs rung (app choice); the status popover sized to
its trigger by re-composing `popover` + `popover_panel` +
`menu_item_checked` with `rect_of_key` and a headless fallback
(library gap, matches item 2 above); the empty inspector centred with
an icon (app choice); `.scrollbar_gutter()` on the book list after a
24-row scene fired `ScrollbarObscuresFocusable` eighteen times
(library default: a scroll of focusable full-width rows is the common
case); and the destructive delete demoted to red text, where its first
reach, `.text_color(tokens::DESTRUCTIVE)`, failed `LowContrastText` at
1.77:1 before it found `DESTRUCTIVE_TINT_FOREGROUND` (token trap: the
fill-grade red is the obvious name, and `.ghost().destructive()` fills
solid rather than giving the red-text variant). It did not notice the
ragged left edges (item 1), and it left the stars as outlines.

Gaps new in this pass: `select_menu` has no trigger-width mode;
`DESTRUCTIVE` as text is a trap the lint catches late; the gutter is
opt-in where focusable rows want it on; `BuildCx::rect_of_key` is
`None` headlessly so measured-rect layout diverges in review; and the
ground-truth PNG path has no copy-in scaffold. On the last: the agent
assembled 170 lines of wgpu boilerplate from the `render_png` example
to screenshot its own app, while `DAMASCENE_HEADLESS_SHOT=out.ppm
cargo run` would have done it. That hook is documented only on a
private function in `damascene-winit-wgpu`, so no README mentions it.

Overlap between the two lists is the signal: the popover width was
found by both the scorer and the agent, the destructive-text trap and
the scrollbar gutter only by an agent that looked, the section inset
only by a scorer measuring the tree. The brief set and the
`blind-rounds` layout are reusable as-is for the next round after the
defaults change.

### Blind round 1 follow-up rulings (2026-10-16)

The gaps above were classified (library defect, documentation, or a
judgment call) and ruled on together. The rulings: the section body
inset goes; filled glyphs come in; the scrollbar gutter follows a
conditional rule rather than an auto-hiding thumb. Each item lands as
its own gated commit, in the order every app hits them.

**Scrollbar gutter by rule.** The metrics pass now reserves
`SCROLLBAR_GUTTER` on any scrollable that paints a thumb and whose
content holds a focusable — the `ScrollbarObscuresFocusable`
condition decided before layout instead of reported after it.
Floating menu surfaces (`SurfaceRole::Popover`) keep the overlay
thumb, the same exemption the lint makes and the 2026-08-22 ruling
that menus do not get a blank band. Content the pass cannot see (the
lazily built rows of a `virtual_list`) still opts in explicitly, which
is what `data_table_virtual` already did; `data_table`'s plain body
keeps its explicit call too, because its header and footer pad to the
same strip and the pairing must hold even when the rows are not
focusable. Chose-because: the gutter is stable (reserved whether or
not the content overflows), so a list never shifts when it grows past
its viewport; rejected-because: an auto-hiding overlay thumb would
remove the need but is a runtime project, and an overflow-only gutter
needs a re-layout pass. The lint stays as the safety net for the two
shapes the rule misses: virtual rows, and a control wider than the
scroll's content rect.

**Select menus floor at the trigger width.** The panel of a popover
can now ask to be at least as wide as its anchor
(`El::min_width_of_anchor`, CSS `min-width: anchor-size(width)`);
the popover layer applies the floor at placement time, where the
anchor's width is first known, and `anchor_rect`'s viewport clamp
still bounds the result. `select_menu` and `select_menu_selected` set
it, so a list of short options stretches to its trigger as a native
`<select>` list does, and a long option still widens the menu past
it. Dropdown, menubar and context menus keep hugging their rows, as
platform menus do. Chose-because: the flag lives on the panel rather
than on `Anchor` — the anchor describes where the panel goes, the
panel describes how it sizes, and `Anchor::Key`'s named fields are
constructible by callers. The blind-round agent's 40-line
re-composition of `popover` + `rect_of_key` with a headless fallback
is what this replaces.

**A field names its control.** `field` and `field_with` now do the
HTML implicit label association in every orientation: the label
becomes the accessible name of the first focusable node inside the
control unless it carries an explicit `aria_label`. `form_item` and
`field_row` already did this through a private helper in `form.rs`;
it moved to `a11y::associate_label` and all three share it. The
placeholder stops masquerading as a label: `text_input` no longer
stamps it into `aria_label`, and both name chains (`accessible_name`
for the lint, the AccessKit lowering for the platform) fall back to
the editable field's placeholder last, after label, content and
tooltip — the HTML-AAM order. Before, the placeholder sat in the
label slot, so a field's label could never reach a placeholder input,
which is the reverse of HTML. The `NoAccessibleName` message now
offers `field(label, control)` as the fix for a form control.

**Quiet variants take a status as colour, not as a fill.** `tint`
now checks whether the node is a quiet variant — `explicit_variant`
set and no fill, which only `.ghost()` and `.outline()` produce — and
on one recolours the text (and an outline's border) with the status
instead of refilling the surface. `button("Delete").ghost()
.destructive()` is the red-text action shadcn writes as a ghost with
`text-destructive`; `.outline().destructive()` is the red-bordered,
red-text button; `.destructive()` alone is still the solid one, and
`.destructive().ghost()` ends muted because the last modifier wins.
`boxed()` calls the filled path directly, since the dot badge it
boxes has no fill yet and may already carry a status. The
`LowContrastText` finding now names the text-grade token when the
failing colour is a fill-grade status token — the trap the agent fell
into with `tokens::DESTRUCTIVE` as text. Open beside this: a bare
`menu_item("Delete").destructive()` still fills the row solid, where
shadcn's `DropdownMenuItem variant="destructive"` is red text; that
wants a menu-item rule of its own.

**`select::apply_event` parses from `&str`.** The parse closure now
borrows the picked token like `tabs::apply_event` and
`toggle::apply_event_single` do; the `String`-by-value form was the
one inconsistency the blind-round agent noted. A `String` field
writes `|s| Some(s.to_owned())`, a number `|s| s.parse().ok()`, an
enum its `from_str`. Breaking, on the 0.9.0 list; eight in-repo call
sites moved and no dependent in the workspace uses it.

**Enter submits; tests can press keys.** `text_input::submitted(&event,
key)` is HTML's implicit submission written once: a focused input
captures keys, so the host delivers Enter to it as a raw `KeyDown`
rather than the `Activate` a button gets — the agent found this by
reading `runtime.rs`. Plain Enter only; a modifier or an auto-repeat
falls through, and `apply_event` ignores Enter so the two calls can
run in either order. `UiEvent::synthetic_activate` and
`UiEvent::synthetic_key_down` join `synthetic_click` for tests; the
core README's testing paragraph and the top-level README name all
three. The catalog gains the Enter row and the picker rule of thumb
(two to four short labels as a `toggle_group`, longer lists as a
select). A `form()` with implicit submission across its inputs stays
open as a separate ruling.

**The headless shot is documented and writes PNG.**
`DAMASCENE_HEADLESS_SHOT` had a doc comment on a private function and
no README; the agent assembled 170 lines of wgpu boilerplate to get a
screenshot. The `damascene-winit-wgpu` README now has "Screenshots
without a window", the top-level README a table row and a paragraph
in the per-app section, and the hook writes PNG when the path ends in
`.png` (PPM otherwise), with the frame-zero caveat spelled out:
`BuildCx::rect_of_key` is `None` headlessly, so a measured-rect layout
shows its unmeasured branch. Found on the way, by shooting the counter
example: `Headless::render_rgba8` handed back the attachment's blend
coverage as alpha, so a PNG written from it showed hollow glyphs on a
blank page in any compositing viewer; the readback is now opaque as
its doc says, and the render test asserts it.

**Section bodies share the title's left edge.** `section_body` had
`pl = SPACE_2` with no stated reason, and it was the inset that gave
the blind-round pane four different left edges (16, 24, 26, 30). Ruled
out; tab and item paddings are canon and stay. The anatomy, the test
and the showcase renders move with it.

**Filled glyphs.** `icon("star").filled()` swaps a built-in for
`IconSource::Filled(name)`: the same Lucide markup parsed with its
closed shapes filled in `currentColor` on top of the stroke, cached
beside the outline asset and keyed separately in the MSDF atlas
(`IconKey::Filled`). The SVG export fills in the stroke colour and
labels the glyph `star filled`; the text-only fallback keeps the
name's glyph; a custom SVG keeps its authored paint. A GPU render test
checks the filled star lights its interior and writes the pair for
the eye. Open strokes (a check, a chevron) fill their implied closure,
as they do in a browser — the modifier is for glyphs drawn with closed
shapes. A stock `rating` widget is the natural next step and is not
in this batch.

### Blind round 2 (2026-10-16, library at 09df4b9)

Same brief, same prompts (saved in `blind-rounds/PROMPTS.md`), a fresh
agent, project `blind-rounds/2026-10-16-readinglist-r2` (`v1` 7261d60,
scoring shots f6b34fb). The delta is the follow-up batch.

**Scorecard, v1 against round 1's v1.** Tool uses 59 (61); tokens
223k (263k); wall time 12 min (15); cargo invocations 9 (14); compile
errors 0 (0, with two test-side errors last time, none now); lint
findings one kind (nine findings of three kinds): `FocusRingObscured`
on every stock `sidebar_menu_button` inside `sidebar_content`, which
the agent cleared by dropping the wrapper. Zero raw colours, fonts or
radii again; one raw size (the inspector's 340) plus the window.

**What the batch bought, visible in the code and the renders.** Enter
submits through `text_input::submitted`; the behaviour tests drive
keys with `synthetic_key_down` and `synthetic_activate`; the rating
stars are `icon("star").filled()`; status is a `toggle_group` (the
picker rule); Delete is `.ghost().destructive()` and renders as red
text; the inspector's `field`s name their inputs, so no
`NoAccessibleName`; a 30-row list reserved its gutter and linted clean
with nothing called, where round 1 fired eighteen findings and needed
the modifier; the middle pane's left edges measure 288 / 288 / 290
(title, inputs, rows) against round 1's 16 / 24 / 26 / 30.

**What it did not buy.** The composition: the agent followed the
core README's "two-pane workbench" skeleton to the letter —
`page([row([sidebar(..), column(..), card(..)]).gap(SPACE_4)])` — and
got three floating panels inset from the window, the dashboard shape
the October review named. The sidebar carries card chrome (fill,
stroke) and `page()` pads around it, so the README's canonical shell
is the cards-as-panes look; round 1's single flush pane read more like
a tool. The rating control is five ghost icon buttons, which leaves
the lit stars muted grey. The list rows stop 12 px before the
quick-add row's right edge, the stable gutter made visible. The
quick-add inputs have placeholders for labels (named, now, through
the placeholder fallback and an explicit `aria_label`).

**Library gaps found by the agent and reproduced here at runtime.**
(1) `sidebar_content` clips and nothing in the stock anatomy reserves
`RING_WIDTH` horizontally, so the sidebar.rs module example lints
dirty — three findings in a probe. `item_group` and `form` bake that
inset; `sidebar_content` should. (2) `text_input::apply_event`'s doc
says every input's call can be dispatched unconditionally; the body
gates only pointer events, and a `TextInput` routed to input "a" is
folded into input "b" as well (probe: both became `"x"`). The runtime
routes key and text events to the focused node's key, so gating them
on route is sound and makes the doc true. (3) `examples/src/bin/
text_input.rs` still hand-rolls the clipboard the host already
bridges — the second agent in a row to flag it. (4) `item()`'s rustdoc
never says `.selected()`. (5) No README points at the icon name list
or `all_icon_names`. (6) The README's "Common app shells" teaches the
floating-panel composition.

**Phase 2 (same agent, allowed to look; `v2` b5a2602, 31 tool uses,
about 7 minutes, 288k tokens cumulative against round 1's 41 / 11
minutes / 388k).** It found `DAMASCENE_HEADLESS_SHOT` in the README
this time and wrote no PNG harness; it added a `READINGLIST_SCENE`
env hook that drives the real `on_event` to a canned state before the
shot (the README's "give the app a way to start in the state you
want"), and folded the review scaffold into its `main` behind
`READINGLIST_DUMP` because a bin crate's examples cannot import it.
Six visual changes: sidebar counts as plain muted tabular numbers
(the `.secondary()` chip vanished on the current row); the status
`toggle_group` wrapped in a `row` because the vertical `field`
stretched its hug width to the card (reproduced: a `Size::Hug` child
of any column lays out at the column's width, 340 against a hugging
223 in a row — `Align::Stretch` stretches `Hug` and `Fill` alike, with
no regard for an explicit width; 22 stock widget files declare
`.width(Size::Hug)` explicitly, so the CSS rule "stretch applies only
to an auto cross size" is a ruling, not a patch); lit stars in
`tokens::WARNING`; the empty shelf centred with an icon and a hint;
the centre column given `pt(SPACE_4)` because a bare column beside
two panels sits 16 px higher than their headers; "N books" to "N
titles". Left alone, with reasons: the inspector header duplicating
the fields, the sparse band above Delete, the dark theme, the
three-panel composition (it did not see it as a problem).

Doc defects it found, verified: the top-level README says
`Theme::damascene_light()` is the default and `Palette::default()` is
`damascene_dark()`; the sidebar module's example uses `badge("7")` as
a count, which under the dot-badge default renders a dot and a
number; the README tells a bin crate to copy `review.rs` into
`examples/`, which cannot import the bin's items; the README shells
show a bare column beside panels with the ragged top. It also asked
for a `rating` control, the second agent to write five star buttons
by hand.

**Round 2 gap list, for ruling.** Library: (a) `sidebar_content`
ring inset; (b) `text_input::apply_event` route-gates key and text
events; (c) explicit `Hug` survives a stretching column; (d) a
`rating` widget; (e) `examples/text_input.rs` clipboard code (cleanup,
flagged twice). Docs: README theme default, sidebar count idiom, the
review scaffold in a bin crate, `.selected()` on `item`, the icon name
list, and the shells — which is the composition question: whether the
canonical workbench stays `page + sidebar card + card` or becomes the
flush shell the hero uses.

### Round 2 rulings (2026-11-11)

Ruled together, as assent to four recommendations: "hug vs stretch:
yes, shell: yes, destructive: yes, El::icon_name: yes. Let's proceed
with the order you outlined." They are checkpoints rather than argued
rulings, and cheap to reopen. Each item is its own gated commit
(`385f2d4` through `aa7f772`).

**A written `Hug` is fit-content.** `Align::Stretch` stretched `Hug`
and `Fill` alike, so `.width(Size::Hug)` on a column's child did
nothing. It now passes over a `Hug` the author wrote (the existing
`explicit_width` / `explicit_height` flags): content size, capped at
the container, at the start of the cross axis. An unset `Hug` and
`Fill` stretch as before, on both axes. Chose-because: it is CSS's
rule (stretch applies to an auto cross size), the flags already record
the distinction, and one modifier answers "this control should not
span the column". Rejected-because: a `Size::Auto` variant would say
the same thing with a public enum change and a new default to teach;
a width-only rule would leave `.height(Size::Hug)` meaning nothing in
a row.

The rule could not land alone. The constructors restated `Hug` freely
while it meant nothing — `text()` and the headings called `.hug()`,
and some 150 sites said `.height(Size::Hug)` — so applied naively
every text leaf would have stopped spanning its column and bare panes
in a row would have stopped matching heights. The audit separated the
two meanings: `text` / `h1`–`h3` / `paragraph` and 82 block-container
sites drop the restatement and are auto; content-sized controls keep
theirs and now hold their width in a column (`toggle_group`,
`button_group`, `badge`, `kbd`, `calendar`, `menubar`, `breadcrumb`,
`pagination`, the floating panels); table cells take
`.height(Size::Fill(1.0))`, because the content El is the cell and may
bring a rung height (a badge's) that the explicit size has to replace
while the cell still spans its row. Measured on the showcase's layout
trees, before against after: 32 nodes move in 6 of 48 scenes. The two
visible ones are the calendar and the menubar, which now hug.

**The workbench shell is full-bleed.** The crate README's two
workbench skeletons are `overlays(row([sidebar, column, sidebar]),
layers)` — panes edge to edge, each with its own padding, the
inspector a right-hand `sidebar` of `section()` regions — and `page()`
is described as the single-pane screen. `tests/readme_shells.rs`
builds both through the prelude and holds them to a clean lint report
and flush geometry. Chose-because: both blind builds copied the README
skeleton to the letter, so the skeleton is where the dashboard shape
came from.

A correction to the round-2 gap list above, which called this "the
flush shell the hero uses": the hero does not use it. Its root is
`row([nav_rail(), main_panel(), inspector()]).gap(SPACE_4)
.padding(SPACE_4)` — three floating panels on a margin, the
composition the README now warns against. The flush shell in the
repository is the showcase's and the workbench examples'. Whether the
hero is recomposed, and whether `sidebar()` grows shadcn's variants
(flush with one inner border by default, `floating` as the opt-in)
instead of stroking all four sides, are open and are the user's.

**A status on a menu row colours the label.** `menu_item("Delete")
.destructive()` filled the row solid red. A row with
`MetricsRole::MenuItem` now takes a status as the colour of its label
and glyph; a muted shortcut hint stays muted. Hover stays the neutral
row hover (shadcn tints it `destructive/10`; not done).

**`El::icon_name` is removed.** No caller in the repository or in the
workspace's dependents.

**Without a ruling, because they were defects.**
`text_input::apply_event` and `text_area::apply_event` handle only
events routed to their key (they gated pointer events and folded every
key and text event; a key pressed with nothing focused moved every
caret), and `UiEvent::synthetic_text_input` joins the synthetic
constructors. `sidebar_menu_button` and `sidebar_group_action` draw
their focus ring inside — the stacked-row recipe — instead of the slot
gaining a 2 px inset that would misalign it with the header and still
clip under the recommended `scroll`. The three text examples lose
their own clipboard layers. The five doc defects are fixed.
`CollapsedFillChild` also reports a spacer in an unstretched hug
container (#151's second shape). html5ever is at 0.40.

**Looked at and left.** #153 (a translucent fill lightening an app
surface under HDR) does not reproduce in the renderer, at the reported
commit or at main, on SDR or float targets: the fills darken by
exactly `1 − alpha` and the frame's alpha is 1.0 throughout. The
measurements are on the issue; it stays open as an observation of the
live output. A password field's Ctrl+C copies the bullets the field
shows (never the value) and its Ctrl+X deletes the selection, because
the host's clipboard bridge does not know the field is masked — a
browser does neither. `accesskit_ios` 0.2.2 still pins objc2 0.5 and
`fdsm` still pins nalgebra 0.34.

**Added to the 0.9.0 breaking list.** Layout: a written `Hug` no
longer stretches (visible: `menubar`, `calendar`, a boxed `badge`,
`kbd`, `toggle_group`, `button_group` in a column; any app node with
an explicit `Hug` on a stretching parent's cross axis). `text_input` /
`text_area` `apply_event` ignore unrouted events. Sidebar rows: inside
focus ring. Menu rows: status as text colour. `El::icon_name` gone.
`CollapsedFillChild` fires on dead spacers.

**Still open.** The hero's composition and the `sidebar()` variants
(above); a `rating` widget (asked for by both blind-build agents);
`form()` with implicit submission; a mask-aware clipboard bridge; the
bottom navigation bar (#152).

### The hero, and the three things it exposed (2026-11-11)

The hero was recomposed as a tool shell (`0e85172`; the user: "This is
much better; let's commit it."): rail, main pane and inspector edge to
edge with one hairline between neighbours, the three metric cards one
strip, the regions `section`s, the queue `item` rows, the inspector a
right-hand sidebar. Parts 1 and 2 above call the old hero the
counterexample to the dashboard look "when a person composes the
page"; it was a good dashboard, and the claim it supported — that
stock damascene can be composed with hierarchy and restraint — stands.
What changed is the genre the library's lead image argues for.

Recomposing it ran into three library gaps, ruled together ("Agreed
with all of these -- proceed.", assent to the recommendations; cheap
to reopen) and landed in this order.

**A section given a height shares it with its body** (`350ab9c`). The
body is `Fill`-height: its content height in the usual hugging
section, what the header leaves in a section given `.height(..)`. No
new API. Chose-because: a `Fill` child of a hugging column already
resolves to its content, so the one change serves both cases;
rejected-because: an opt-in `SectionOpts::fill()` would be a second
way to say `.height(Size::Fill(1.0))`.

**`sidebar()` is a flush pane; `side` and `variant` are shadcn's**
(`e96c99f`). Flush is the default: card fill, no surface role, one
per-side hairline on the inner edge (`SidebarSide::Left | Right`),
its pixel taken out of the padding so the content box does not move.
`.floating()` restores the Panel frame and shadow. Found on the way:
the Panel role *sets* its stroke uniform, so an authored
`.stroke_width(0.0)` on a Panel node does nothing — the hero's first
override looked right only because the frame sat on the window edge.
Left as it is (cards depend on the role's stroke); a flush pane simply
takes no role.

**`clip_margin`** (`5cc267c`). CSS `overflow-clip-margin`:
a clipping node lets descendants paint a margin past its rect; on a
scroll, on the cross axis only. `scroll()`, the virtual lists and
`sidebar_content` carry `RING_WIDTH`, and the insets that `form`,
`item_group`, `field_group` and `field_set` baked to make ring room
are gone, with the showcase's hand-padded scrolls. Chose-because: the
ring gets its room where it is cut, and nothing is nudged 2 px to make
it — the inset was the ragged-left-edge class both blind rounds
measured. Rejected-because: a pad-by-rule (the scrollbar gutter's
shape, applied to rings) keeps the misalignment and only automates it;
an automatic inside ring changes how one control looks depending on
its container. Rendered before and after with a focused field flush
in a scroll: the ring's sides are cut at the rect and whole with the
margin.

**Added to the 0.9.0 breaking list.** `SidebarOpts` gains `side` and
`variant` (struct-literal construction breaks); a sidebar floating in
a padded page needs `.floating()`. `form` / `item_group` /
`field_group` / `field_set` lose their 2 px side insets. `El` gains
the public `clip_margin` field. The hero image and canvas (1360×900).

**Still open.** The vertical-axis ring paddings left in the showcase
(a row of buttons under a horizontal scroll, a slider above a
paragraph) are occlusion and main-axis cases the margin does not
address. `SurfaceRole::Panel` overriding an authored stroke.

### The open list, worked through (2026-11-11)

The fourteen commits above were pushed (the user: "Let's push this and
work on the remaining items."); GitLab pipeline 654 ran all seven jobs
green on `4aee945`, including the MSRV and iOS jobs that cannot run
locally. #151 is closed. What followed, one commit each:

**A password field neither copies nor cuts** (`d784ed4`). The stock
hosts bridge the clipboard without knowing which field is masked, so
Ctrl+C copied the bullets and Ctrl+X deleted the selection.
`EditableText` gains `password`; `text_input` sets it for a masked
field; `selection::selected_text` resolves to `None` inside one (copy
and the X11 primary selection both read through it); and
`RunnerCore::clipboard_request` reports no Copy or Cut for an event
routed to one. The winit, web and ash hosts match on that instead of
`text_input::clipboard_request`. The same flag lowers the AccessKit
role to `PasswordInput`. Chose-because: the tree already knows, so no
app has to tell its host which fields are secret. Rejected-because: a
host-side list of masked keys is a second source of truth.

**Panel and Popover frame by default** (`796a307`). Both roles wrote
their stroke colour and width over the node's, so
`card(..).stroke(PRIMARY)` and `card(..).stroke_width(0.0)` did
nothing — the trap the hero override fell into. They are defaults
now, as `Input` and `Sunken` have been since 0.7. Measured with a
draw-op diff over all 48 scenes and a pixel diff over the 22 section
renders: the eight floating recipes declared `BORDER` and were painted
`INPUT`, so they now declare `INPUT` and nothing floating moves; stock
cards and alerts paint `BORDER` as declared instead of `BORDER` at 82%
(at most 4/255 on a hairline); and the status-tinted alerts gain the
status-coloured hairline their tint always set and the role always
discarded — the one visible change, five alerts on two pages.
`Selected`, `Current` and `Danger` still override: they are states,
and dropping an outline is what they are for.

**The last ring paddings** (`edfd4de`). A correction: the section
above called the four vertical ring paddings left in the showcase
"occlusion and main-axis cases the margin does not address". Three
were horizontal scroll strips, whose cross axis is vertical, so the
clip margin already covered them; the fourth, a wrapper under a
slider, was not needed at all. All four are removed and the 48 scenes
lint clean. The scissor test gains the row-axis case.

**`rating`** (`6ce4b84`), asked for by both blind builds.
`rating(key, value)` is the WAI-ARIA rating radio group: ghost icon
buttons, every star up to the value filled, the value's own star the
checked radio, arrows moving among them. The value is a `u8`, `0` for
unrated; `rating::apply_event` folds a pick and picking the current
star clears it (MUI's behaviour, and the only way to take a rating
back without a second control). A size rung or a status on the row
reaches the stars. `rating_display(value, max)` is the read-only score
for a list row. Not built: fractional display (an average of 3.5),
hover preview of the stars up to the pointer.

**`navigation_bar`** (`e963630`, #152; renamed `bottom_navigation`
before release — see "Two rulings before the release check" below). The phone's bottom bar, with
the four things the app-side version had to find built in: a
stretching item column so a long caption ellipsizes, no sideways hit
outset, an inside focus ring, and the accent on the glyph only. It
routes `{key}:tab:{value}` and re-exports `tabs::apply_event`, so it is
interchangeable with `tabs_list` at the handler. The bar hugs its
height, so `.pb(cx.safe_area_bottom())` extends it under the home
indicator; no flag was added for that. The name is Material 3's (and
the issue's); iOS calls the *top* bar a navigation bar, which the docs
say.

**Added to the 0.9.0 breaking list.** `EditableText` gains `password`
(struct-literal construction breaks). A node with the `Panel` or
`Popover` role and its own stroke now paints that stroke: an app that
copied a stock floating recipe's `.stroke(tokens::BORDER)` moves from
`INPUT` to `BORDER`. Status-tinted alerts have tinted borders.

**Still open.** `form()` with implicit submission (ruled and built
the same day — next section), which this record
reserves for a ruling: the runtime can do it (`UiEventKind` is
non-exhaustive and `key_down`'s capture path already returns a list of
events and can read the focused node's ancestors), so the question is
shape — a `Submit` event routed to a keyed form, HTML's `onsubmit`,
against a pure helper that takes the input keys. The destructive menu
row's hover tint (shadcn's `destructive/10`), which wants a status
branch in the hover wash. The hero image is 5/255 stale on a few
hairlines and was not regenerated.

### `form()` is submitted as a whole (2026-11-11)

**The ruling.** Put to the owner as two shapes — a `Submit` event
routed to a keyed form (recommended), or a pure helper taking the
input keys and the button key. His words: "a submit event on the form
sounds fine, and the helper also sounds fine. Push when the changes
are in." Bare assent to both, so a checkpoint and cheap to reopen. My
reading: either was acceptable and the choice was mine. Built: the
event, with `form::submitted(&event, key)` as its helper. Not built:
the key-list helper — rejected-because it is the thing the event
replaces (its list of fields is what drifts when a field is added),
and shipping both would give an author two ways to write one check.
`text_input::submitted` stays for a lone field with no form around it.

**The shape** (oracle: HTML — `<form onsubmit>`, `<button
type="submit">`, the `submit` event, the spec's *form owner*; row
added to `docs/NAMING_ORACLE.md`).

- `form(children).key(key)` — the key is where the submission is
  routed, and what makes the form submittable; without one it is
  layout only. (First built as `form(key, children)`; see "The key
  moved to `.key()`" below.)
- `.submit()` on a control makes it a submit button; `.form_owner()`
  is the public marker `form` sets, so a custom container can be a
  form (symmetry rule).
- `UiEventKind::Submit`, routed to the form's key, delivered right
  after the event that caused it: the button's `Click` / `Activate`,
  or the field's Enter `KeyDown`. `UiEvent::synthetic_submit(key)` for
  tests — a test that calls `on_event` directly sends the `Submit`
  itself, since the runtime is what synthesizes it.
- Enter submits from a single-line field only: a node that declares
  `editable_text` without `multiline` (`text_input`, `numeric_input`;
  not `text_area`). Plain Enter, no modifier, no auto-repeat — the
  predicate `text_input::submitted` already used, now shared as
  `UiEvent::is_plain_enter`.
- HTML's default-button rule is taken: the form's first submit control
  in tree order is its default button, and while it is `.disabled()`
  Enter submits nothing.
- Lint `DeadSubmit`: a `.submit()` control with no keyed form above
  it — outside any form, or inside one that was never keyed. It is
  "press it and nothing happens", which a headless author would not
  see.

Where it lives: `crates/damascene-core/src/submit.rs` holds the tree
rule; `RunnerCore::key_down`, `pointer_up` and `accessibility_action`
pass their events through it. Hosts are untouched.

**Divergences from HTML, recorded.** A button is a submit control
only when marked (HTML's default `type` inside a form is `submit`,
which would make every Cancel submit). No `submitter` on the event:
the button's own `Click` precedes the `Submit` for an app with two
submit buttons. HTML's rule that a form with several fields and no
submit button ignores Enter is not taken. Enter on a focused checkbox
or switch still toggles it, as before.

**Consequences.** Nothing breaks for an existing `form([...])`: it
compiles and behaves as before. Three dependents in the workspace
call `form(...)` — whisper-agent, whisper-git (its clone / branch /
merge dialogs, which are the forms this is for) and rumble; a
`.key(..)` on the form and `.submit()` on the dialog's primary button
buy them Enter. A keyed form is a hit-test target like any keyed
container, so a press on its empty space is routed to its key rather
than to a keyed ancestor; it carries `.no_hover()` and takes no
focus. `El` gains two public `bool` fields. Additive:
`UiEventKind::Submit`, `FindingKind::DeadSubmit` (both enums are
non-exhaustive).

**The key moved to `.key()` (2026-11-11, same day).** First built and
pushed (`b5464f7`) as `form(key, children)`. The trial upgrade of
whisper-agent against that commit found 40 `form(...)` call sites,
every one a layout stack with nothing to submit, and each would have
needed an invented key to compile. Put to the owner with a
recommendation to revert; his words: "Agreed with going back to
form(children)". Chose-because: the signature stays what 0.8.0
shipped, so the 40 sites compile untouched; and it is the rule the
rest of the kit already follows — `button(label)` is a button, and
`.key(..)` is what makes it clickable. Rejected-because, for the key
parameter: it made every form pay for submission whether or not it
had anything to submit. What the parameter bought and this gives up:
a form with fields and no key now compiles and silently does not
submit on Enter. The lint still catches the case with a `.submit()`
button in it, and names the missing key.

With the key optional, an unkeyed form owner had to mean something
when nested: it is passed over. A control belongs to its nearest
*keyed* form owner, so a `form([...])` used as a layout stack inside
a keyed form does not swallow its fields' Enter, and the lint no
longer flags a bare `.form_owner()` without a key.

**In the showcase.** The Forms page is one keyed form: Save is its
submit button, Enter in the name or email field submits, and the
outcome shows beside the buttons until the next edit. Its `on_event`
checks `form::submitted` once. `examples/text_input` does the same.

**Left.** `input_otp` captures keys but declares no editable text, so
Enter in it does not submit its form. The destructive menu row's hover
tint and the stale hero image, as above.

### Two rulings before the release check (2026-11-11)

Asked what else to settle before 0.9.0, I put two API questions to the
owner. His words: "1: agreed with #[non_exhaustive] where it makes
sense. 2: we can rename navigation_bar now if you think it makes
sense". Both delegate the judgment; both are recorded here with the
judgment made.

**`navigation_bar` is now `bottom_navigation`.** Chose-because: the
name has one meaning in every corpus an author brings (Material 2 /
Compose `BottomNavigation`, MUI, Android `BottomNavigationView`,
Flutter `BottomNavigationBar`) and it states the position, which is
what the widget is. Rejected-because, for the Material 3 name it
shipped under: a web author's "navigation bar" is the header across
the top of a site, iOS's is the title bar, and this crate's own
safe-area docs already use the phrase for Android's *system* bar —
the one the widget is padded to clear. `tab_bar` (iOS, Ionic) collides
with `tabs_list` and `editor_tabs`. Renamed with it:
`bottom_navigation_item`, `bottom_navigation_from_items`,
`BOTTOM_NAVIGATION_HEIGHT`, the module, the inspector kinds. Nothing
released carried the old name; #152's closing comment did, and gets a
follow-up when this is pushed.

**`#[non_exhaustive]`, and where.** The semver check against 0.8.0
failed on fields added to `Palette`, `TextInputOpts`,
`NumericInputOpts` and `EditableText`; 0.8.0 had failed on `Palette`
too. The rule applied: *a struct an app fills in to configure
something, which has a builder for every field, is non-exhaustive* —
so a field added later is a minor release.

- The eleven widget option structs: `TextInputOpts`,
  `NumericInputOpts`, `TextAreaOpts`, `FieldOpts`, `MeterOpts`,
  `ScrubberOpts`, `ResizableOpts`, `SidebarOpts`,
  `SidebarMenuButtonOpts`, `DataTableOpts`, `TreeItemOpts` (joining
  `RatingOpts`), and `EditorTabsConfig`, which gained its two builders.
  The seven that are `Copy` got a `const fn new()` and `const`
  builders, so an options value can still live in a `const`
  (`examples/text_input` does this).
- `Palette`: a custom palette is now a stock one plus assignments
  (`let mut p = Palette::damascene_dark(); p.primary = …;`). The three
  in-repo palettes that used `Palette { …, ..base }` moved; the values
  are unchanged.
- `EditableText` (with `EditableText::new(value)` and builders) and
  `A11yProps`.
- `HostConfig`, `WebHostConfig`, `HtmlOptions`, `MarkdownOptions`.

What an app loses: `Opts { field, ..Default::default() }` no longer
compiles outside the crate. Measured in the workspace's dependents
before deciding: eight such literals (rumble six, the Raven-Firmware
kiosk one, kalogon's vendored copy of it one) against 81 uses of the
builder form, so the builder is already the common spelling and the
fix is mechanical. That form survived field additions already; what
the attribute buys is that a *full* literal can no longer be written
and broken, and that the release tooling stops reporting a major
bump for an added option.

Left exhaustive on purpose: value and geometry types (`Rect`, `Sides`,
`Corners`, `Color`, `KeyModifiers`, `Pointer`), the GPU instance and
atlas structs, drag-state structs, and `AccessibilityPreferences`,
whose docs tell hosts to use `..Default::default()`. Not looked at,
and still literal-constructible: the plot and scene specs
(`PlotSpec`, `SceneSpec`, their style structs), `ViewportConfig`,
`ToastSpec`, the table column specs, and `HostDiagnostics` (29
fields, built by each host). The enums `IconSource` and `IconKey`,
which also gained a variant, stay exhaustive so the three backends
keep getting a compile error when one is added.

### Disabled is neutral, then faded (2026-11-11)

**What the trial showed.** In whisper-agent's pod editor a disabled
primary button sat beside an enabled secondary one and the two were
hard to tell apart. Measured afterwards: `muted` is the `secondary`
fill in all eight stock palettes, and `muted-foreground` is a ghost
button's label colour. So the Phase B treatment — `MUTED` fill,
`MUTED_FOREGROUND` label, full opacity — draws a disabled filled
button as an enabled secondary one wearing an enabled ghost's label.
On the default dark palette: fill 39 for both, label 161 against 250.

**The ruling.** Three candidates were rendered in three palettes and
put to the owner with a recommendation. His words: "neutral, then
faded looks fine. Push once that is in". Chose-because: it keeps the
Phase B ruling (no dim primary-coloured block) and adds the one thing
every other disabled node already had, the fade, so a disabled label
lands at one level whether the control is filled or not, and the box,
border and shadow recede with it. Same palette: fill 28, label 119.
Rejected-because: dimming only the label leaves a full-strength box
and border that still read as pressable; the 0.8.0 look (the variant
itself at half opacity) is what Phase B ruled out.

**The rule now**, in `El::disabled` (`theme/style.rs`): every disabled
node takes `MUTED_FOREGROUND` content and is drawn at
`tokens::DISABLED_ALPHA`; a node with a fill or a stroke goes
`MUTED` / `BORDER` first. The fade is `min`, not assignment: a node
already below it (a hidden indicator) stays hidden, and disabling
twice is disabling once.

**Cost, accepted.** In the light palettes a disabled filled button's
label drops from about 4.4:1 to 1.6:1 against its fill — where a
disabled ghost button already was, and near Radix Themes' own
disabled text. WCAG exempts inactive controls. The knob, if it proves
too faint on a real screen, is `DISABLED_ALPHA`.

**Fixed with it: `.opacity()` did not reach role-provided paint.**
`Theme::apply_surface_uniforms` wrote a surface role's default fill
and stroke after the node's opacity had been applied to the authored
ones, so they stayed at full strength. A disabled `text_input`
therefore dimmed its value and kept its trough and border; a card
faded by an app kept its default border. The role's colours now take
the node's opacity. Not fixed, and noted: a drop shadow's strength
does not follow `.opacity()` either (the shader derives it from the
elevation level alone); on the stock controls it is too faint to see.

**Looked at** (enabled beside disabled, page and card, default dark,
default light, slate-blue dark): the button family, `split_button`,
`button_group`, `toggle`, `checkbox`, `switch`, `slider`,
`select_trigger`, `text_input`, `text_area`, both badges. Unchanged
by this and left as is: a disabled switch that is on, and a disabled
slider's range, keep the primary colour at half strength — they are
showing a value, not offering an action.

### The vulkano runner catches up (2026-11-11)

Found while checking the published crates against 0.8.0: the release
notes told a custom host to ask its runner for the clipboard request,
and `damascene_vulkano::Runner` had no such method. It also lacked
`selected_text`, `selected_text_for`, `focused_captures_keys` and
`would_press_focus_text_input`, which the wgpu and ash runners have,
so a vulkano host could not copy a selection or drive an on-screen
keyboard. The owner's words: "let's add the missing components to the
vulkano runner". All five are one-line forwards to `RunnerCore`.

Why it drifted: `backend_runners_share_common_interaction_surface`
(in `damascene-vulkano-demo`) pins the shared surface at compile
time, but its list was not extended as the runners grew, and it
compared only wgpu and vulkano. It now lists every
interaction and lifecycle method the three runners share (22 more
signatures) and includes `damascene_ash::Runner`. Constructors,
shader registration, `prepare` and `render` stay backend-specific.

The vulkano *demo host* then got copy, cut and paste (owner: "We can
implement copy+paste for vulkano next"), the ash demo's handling over
the new forwarders. Its test drives a real `Runner` on a windowless
device against a fake clipboard — copy, cut, paste, and a password
field from which nothing is copied or cut — and skips where there is
no Vulkan.

Left: the vulkano demo never enables the IME (it handles
`Ime::Commit`, which therefore cannot arrive; the ash demo calls
`set_ime_allowed`), and has no primary-selection paste.
Accessibility-tree forwarding (`accessibility_tree_update`,
`accessibility_action`) exists only on the wgpu runner.

### Phone pass for `bottom_navigation` (2026-11-11)

On the owner's OnePlus 8 Pro (Android, gesture navigation; 411 x 905
logical px at 2.625x; safe area top 35, bottom 16), driven over adb
with screenshots. Two builds of the showcase APK from `49ff69d`: the
stock showcase, and a throwaway app with the bar where the docs put
it — full-bleed at the bottom of the root column, five items,
`.pb(cx.safe_area_bottom())`.

- **Showcase demo.** Renders; a tap on an item moves the selection
  and the heading above it.
- **Placement.** The bar's fill runs to the physical bottom edge,
  under the gesture pill; glyphs and captions sit clear above it.
- **Hit regions.** Taps 4 px either side of an item boundary select
  the item they are in; a tap just above the bar selects nothing.
  The safe-area band below the items is padding, not target.
- **Long caption.** "Notification center" among five items
  ellipsizes inside its own item, regular and bold; the neighbours do
  not move.
- **Soft keyboard.** Focusing a text field raises it. Android
  reports the keyboard through the safe area (bottom 16 -> 366;
  `keyboard_inset` stays 0, as its docs say), so the documented
  padding lifts the bar to sit directly on top of the keyboard, and
  Back returns it to the bottom edge. An app that would rather hide
  the bar while typing has no keyboard-specific signal on Android to
  branch on.

Not checked: landscape (side insets), three-button navigation, a
light theme on the device, press feedback under a held finger,
TalkBack, and iOS.


### Independent review of the day's runtime changes (2026-11-11)

Three reviewers, each given the code and a focus (events and forms;
paint and theme; layout, lint and the HTML crate) and not the
conclusions above. Every finding was checked against the code or a
run before it was acted on.

Fixed, each with a test (`7673938`, `fa7a4bf`, `fcef5c8`):

- A control disabled while it holds focus kept receiving Enter, Space
  and typed text; focus is now dropped when the focused node (or a
  container on its path) becomes disabled. Found twice, independently.
- Nothing disabled submits a form — the form, a container, the field
  or the button — and a held key on the submit button submits once.
- A keyed container no longer takes the inflated touch target of a
  child inside it: a press just beside a 16 px checkbox in a keyed
  form went to the form.
- Cut with nothing selected deleted the character after the caret, in
  every host. It now does nothing.
- `input_otp::apply_event` folds a synthetic event by its route; the
  dead-submit lint also catches a `.submit()` control with no key in
  its subtree.
- Web host: soft-keyboard keys carry the DOM repeat flag, and the
  Enter that confirms an IME composition is not a submit. Compiled
  for wasm32; not run in a browser.
- Hovering a role-filled surface (an input trough) no longer replaces
  the fill with a fading accent wash; `.invalid()` keeps the trough;
  `.disabled()` reads the same on either side of a variant modifier;
  a disabled node shows no hover or press; a status on a command row
  colours its label rather than filling the row.
- `damascene-html`: CSS `auto` writes no size, so `width:auto` blocks
  stretch again instead of hugging.
- The focus-ring lint looks past the nearest clip, so a ring cut by
  an outer one (a resizable pane) is reported.

Found and left, at the time:

- **Horizontal scrolling did not exist.** `scroll(..).axis(Axis::Row)`
  laid out a row and clipped it; the offset was only ever applied
  vertically. Fixed below.
- **A drop shadow did not follow opacity**, and was painted under the
  whole silhouette. Fixed below.
- A statically `.scale()`d subtree is hit-tested at its unscaled
  rects.
- The contrast lint does not see a role-provided fill: the placeholder
  on a light-palette input trough measures 3.5–4.2:1 unreported.
- A disabled node inside a disabled non-composite container draws at
  a quarter strength (the two fades multiply).
- `.shadow(0.0)`, and `.stroke_width()` without `.stroke()`, lose to
  the surface role's defaults.
- A ghost button takes the Raised role's border and shadow while it
  is focused.
- Opacity does not reach untinted images, link runs, or vector and
  3D-scene draws.
- `field_with` sizes a bare `text()` value to the control width.
- Spacer-lint false positives (a stacked card; a node with its own
  `max_width`); the section-body lint blames library source.
- A written-`Hug` row of two `Fill` wrap-text columns overflows; an
  unset width with an `Aspect` height measures 0.
- `rating::apply_event` does not clamp; `split_button().primary()`
  shows no seam; nested labels in a menu row miss the status colour;
  a field cannot veto its form's Enter.
- `rect_of_key` returns `None` inside a `virtual_list_dyn` row (seen
  in passing, not investigated).

### A fading surface takes its shadow with it (2026-11-11)

The owner: "Let's fix the horizontal scroll and shadow under fading
surfaces issues now."

The fault, measured on the GPU: a white `SHADOW_MD` surface over a
white page read 208 of 255 at opacity 0 and 233 at 0.5. The fill and
stroke carried the node's opacity in their alpha; the shadow is
derived in the shader from the elevation level and had no way to
hear about it, and it is drawn under the whole silhouette, so it
showed through the fading fill. Every popover, dialog and toast in a
light theme began its fade-in as a grey slab.

The fix is CSS group opacity for the shadow and fill together. The
quad instance gains the node's resolved opacity (`QuadInstance::opacity`,
one `f32` at `@location(8)`, library-owned like `inner_rect`), and
`stock::rounded_rect` scales the shadow by
`(opacity − fill_alpha) / (1 − fill_alpha)`: plain `opacity` outside
the silhouette, zero under an opaque fill, and exactly 1 at full
opacity — so nothing that is not fading changes by a bit, and a fill
that is translucent by design still sits on its darkened ground.
Chose-because: it is exact for shadow-plus-fill and costs one scalar.
Rejected: scaling the elevation level by opacity (levels of 4 and up
all carry 0.10 alpha, so the slab would persist until the last sixth
of the fade); fading the shadow alone by opacity (a quarter of the
slab remains at mid-fade).

**Reversed in part on 2026-11-13** ("A shadow is cast outside its
surface", the last section). The shadow is no longer drawn under the
silhouette at all, so "a fill that is translucent by design still
sits on its darkened ground" no longer holds, and the scaling above
is plain `opacity` on a shadow that is only outside. What this
section fixed still holds: a fading surface takes its shadow with it.

Custom shaders receive the same value and may multiply their own
output by it — the contract is in `paint/shader_contract.md`. The
SVG export mirrors the shader: a faded, shadowed node is emitted at
its authored alpha with an `opacity` attribute.

Breaking for custom hosts: `QuadInstance` has a new field and is 116
bytes; a host with its own pipeline adds the attribute. The three
stock backends are updated, and the stock quad shaders declare the
location (the #138 rule).

Not changed: a stroke or focus ring over a fading fill still
composites a little stronger than a true group fade would, on its
own 1–2 px band. Other stock quad shaders (spinner, skeleton,
indeterminate progress) did not read the opacity. This record said
their colours arrive pre-faded; they do not, and a review the next
day found all three at full strength in a faded subtree. They read
it now (see "Opacity for paint no colour carries").

### A row-axis scroll scrolls (2026-11-11)

`scroll(..).axis(Axis::Row)` laid its children out in a row and
clipped them, and nothing else: the offset was measured, clamped and
applied down the page only, so content past the right edge could not
be reached by any input. Four showcase strips were written against
it (the math and HTML preset chips, the editor-tab strip, the media
tiles), with comments saying users could swipe them.

A scrollable now scrolls along its own axis — down a column, across a
row-axis scrollable (`El::scrolls_horizontally`; virtual lists always
scroll down) — and every path that moves or reads an offset knows
which:

- **Layout.** The content extent, the clamp and the shift are taken
  along the scroll axis (`apply_scroll_offset`; `shift_subtree`
  replaces `shift_subtree_y`). `pin_start` / `pin_end` work on either
  axis.
- **Wheel.** `pointer_wheel` takes `(dx, dy)`. Each component goes to
  the innermost scrollable under the pointer that scrolls along it and
  can still move, independently — the DOM's rule, so a plain mouse
  wheel over a strip scrolls the page behind it. Shift turns a wheel
  that reports no horizontal motion sideways, as browsers do. The zoom
  surfaces (3D scene, `viewport()`, plot) still take `dy` first.
- **Touch.** A drag is offered to the axis it mostly travels (ties to
  vertical, the platforms' bias), then to the other if nothing there
  can move, so a sloppy sideways swipe on an ordinary page still
  scrolls it. Once a scrollable takes the gesture it keeps it, along
  its axis: cross-axis drift moves nothing, and at its edge the motion
  chains only to an ancestor that scrolls the same way. Inside a
  `viewport()` an inner scrollable is offered the drag only when the
  motion runs along its axis (the #130 rule, now asked of the
  scrollable instead of assumed vertical). Momentum follows the
  scrollable it started on.
- **Thumb.** A row-axis scroll shows its thumb along the bottom, with
  drag and click-to-page along x. Where the content holds a focusable
  the metrics pass reserves the band as bottom padding (right padding
  for a scroll that scrolls down), and `ScrollbarObscuresFocusable`
  checks the bottom band for a row-axis scroll.
- **Focus.** Tab, arrow navigation and the AT `ScrollIntoView` action
  reveal a target sideways through a row-axis ancestor
  (`scroll_into_view` tracks drift on both axes).
- **Accessibility.** A row-axis scroll reports `scroll_x` with
  `ScrollLeft` / `ScrollRight`; the others `scroll_y` with
  `ScrollUp` / `ScrollDown`. An action for the other axis is ignored.

Deliberately vertical-only: scroll anchoring (CSS anchors in the block
direction only), `ScrollRequest::EnsureVisible` (it names a `y..y+h`
range; a row-axis scroll leaves the request for a scroll that can
answer it), the layout prune for far-off children, `UiTarget::
scroll_offset_y`, and virtual lists. A node scrolls along one axis;
panning in both is `viewport()`.

Chose-because, on the wheel: routing the two components independently
is what the DOM does and needs no heuristic. Rejected: sending only
the dominant component, which would stop a trackpad's small vertical
drift from nudging the page during a sideways swipe, but alternates
targets near 45° and has no web precedent.

Showcase: the two phone-only chip and tab strips opt out of the thumb
(`.no_scrollbar()` — they are swiped, and stay as tall as their
chips, so their bundles do not change). The HTML preset strip, which
a desktop pointer also meets, keeps it; its label takes the same
bottom band so the pair still centres on the buttons. The media tiles
keep the default.

Breaking: `pointer_wheel(x, y, dx, dy)` on `RunnerCore` and the three
runners, and `UiState::pointer_wheel(root, point, dx, dy)`.
`ScrollMetrics` gains `horizontal`, and `viewport_h` / `content_h`
become `viewport_extent` / `content_extent`; `ThumbDrag::
start_pointer_y` becomes `start_pointer` with `horizontal`. Both
types are public only by re-export — no public function returns
either.

Verified by tests through `RunnerCore` (wheel, Shift + wheel, touch
commit and axis lock, thumb drag and paging, Tab reveal, the
accessibility actions) and by the showcase render. Not tried on a
device: the phone was not attached, so a real swipe on Android and a
real trackpad on the web host are untested.

### The review's left list, worked through (2026-11-12)

The owner: "horizontal scroll seems to work. Let's work through the
remaining left issues." Three commits (`346715a` paint and theme,
`afadb19` layout and lint, `2dd665c` events and hit-testing), each
fix with a test, then a fourth for what an independent review of
those three found (below). Across all four, the 48 showcase scenes'
artifacts (SVG, tree, draw ops, lint) are unchanged but for a library
line number in two tree dumps.

**Paint and theme**

- **An authored value beats the role's default.** `.shadow(0.0)`
  wrote no uniform, so the role filled its own tier back in, and
  `.stroke_width(..)` was written only beside an authored colour.
  Both are written now; `El::explicit_stroke_width` tells an
  asked-for `0.0` from an unset one. `card(..).shadow(0.0)` is flat;
  `text_input(..).stroke_width(0.0)` is borderless.
- **A decorative role frames a surface the node painted.** A ghost
  button paints none: its quad exists only for the focus ring or the
  hover wash, and the `Raised` role was framing that quad — a border
  and shadow appearing on keyboard focus. Where a node authors no
  fill and no stroke and its role provides no fill, the quad is
  painted with no role. Rejected: clearing the role in `.ghost()`,
  which a later `.secondary()` would have had to restore.
- **Disabled is a state, not a stack of fades.** A disabled control
  under a disabled row multiplied to a quarter strength. The paint
  walk carries the disabled share of the inherited opacity and a
  disabled node joins it by `min`. Authored opacities still multiply.
  Chose-because: it holds through an eased fade (a control disabled
  while its row already is stays at the one strength). Rejected:
  treating the nested node's opacity as 1, which flashes to full
  strength when the outer node is disabled second.
- **Opacity reaches paint that had no colour to carry it:** an
  untinted image (through a white tint), an inline link run, a
  one-colour vector mask. A standalone `text(..).link(..)` was worse
  than unfaded: the recorders replaced its colour with the raw
  `LINK_FOREGROUND` token after palette resolution, so it ignored the
  palette as well. The op now carries the link colour.
  Painted vectors, app textures and 3D scenes were left here, their
  colours living in the asset, the texture and the scene; they are
  done in the section after this one.
- **A variant on a whole joined group re-resolves its seams.**
  `split_button(..).primary()` gets the divider
  `button_group([a.primary(), b.primary()])` has; a disabled group
  loses the stale one.
- **A status on a menu row follows the label through a layout
  wrapper** (a label stacked over its description) and leaves a
  painted child or a control — a badge, a nested icon button — alone.
  The second half changes an existing case: a badge directly in a
  destructive row used to take the status colour as its text.
- **The contrast lint reads text against a role-provided fill**
  (`Theme::role_fill`), not the page behind the trough.

**What the lint then showed, and what was already there.** The gate
lints the showcase under the default dark theme only. Swept across
all eight stock palettes, the four dark ones are clean and the light
ones are not:

| Palette | `LowContrastText` | of which on a trough |
|---|---|---|
| damascene-light | 34 | 2 |
| radix-slate-blue-light | 44 | 12 |
| radix-sand-amber-light | 44 | 12 |
| radix-mauve-violet-light | 12 | 12 |

The trough findings are new to the lint and are all on the typography
page (`muted-foreground`, `accent-foreground` and
`info-tint-foreground` at 3.2–4.3:1 on a light `Sunken` / `Input`
fill, which is `muted` darkened 8%). The rest predate this work:
`muted-foreground` on `muted` / `accent` at 4.40:1 in damascene-light,
`accent-foreground` on `accent` at 4.25:1 in slate-blue and sand-amber
light. Not changed in this pass: these are palette values, and the
palettes are the owner's rulings. Ruled and fixed 2026-11-13; see
"Light palettes hold 4.5:1" at the end.

**Layout**

- **A written `Hug` on a row is fit-content, measured as it is
  placed.** Its `Fill` children were measured at their share of the
  available width while the row reported the narrower sum of what
  their content made of it; placed at that width, a long wrapped
  column was re-wrapped into a rect taller than the row. The row now
  budgets its fills from their max-content widths, capped at the room
  on offer, and measures each at its share — CSS's rule for a
  `fit-content` flex row. Rejected: applying it to every row placed
  at its content width (an unset `Hug` under `Align::Start`, in a row,
  in an overlay): the same fault is there and is older, but that needs
  the parent to tell the sizing pass how the child will be placed, and
  changes the measured width of rows in shipped apps.
- **An `Aspect` height measures against the width the node is
  placed at.** Under an unset width it measured against the content
  width — zero for an empty tile — so a column placed the tile 400×200
  over a wrapper of height 0. The sizing pass now carries whether a
  node will be placed at exactly the room on offer (`definite`: it is
  `Fixed`, or stretched or given a `Fill` share all the way up). Where
  it is, an auto width takes that room as the aspect's basis; where it
  is not — a written `Hug`, a centring column, an overlay — the column
  corrects its stretched aspect children against its own content width
  once it knows it. The first version of this fix measured against the
  room on offer whenever the column's alignment stretched, and
  reserved half the page under a tile a few pixels wide in a centred
  column; the review caught it.
- **`field_with(.., control_width)` leaves a plain `text()` value at
  its content width**, ending at the trailing edge. It did so through
  0.8 because `text()` carried a written `Hug`; when that was dropped
  the value was pinned to the control width and started at the control
  column's left edge instead. Under `inline_label` it takes the
  remainder, as a control does.
- **Dead-spacer rule.** A container held at its own `max_width` /
  `max_height` is not hugging for want of a stretch. A card whose
  spacer lines its footer up beside its neighbours is still flagged
  where the same cards are stacked — the rule sees one arrangement —
  and the advice now names that case.
- **A section's body** is marked library-made. A finding on it is
  attributed to the `section(..)` call, and a hug section with next
  to nothing in its body is no longer a `CollapsedFillChild`: a
  wrapper a widget made `Fill`, with no ink and nothing anywhere
  inside it that fills, lost nothing. A body holding something that
  fills — directly, or a level down in a row — is still reported.
- **A clip collapsed to zero width or height stays shut**; its
  `clip_margin` reopened a 4 px sliver of the hidden content. (Found
  by the review, missing from the list above.)

**Events**

- **A press follows a scaled subtree.** Paint scales a node's whole
  subtree; the hit-test walk scaled each node's own rect only. The
  five walkers — targets, links, text selection, scroll routing,
  overlay occlusion — carry the transform paint uses
  (`PaintXf::enter`).
- **`El::blocks_implicit_submission()`**: plain Enter in the field,
  or any field under the node (a wrapper, a whole form), does not
  submit its form. For a field with its own use for the key; set per
  build, so it can follow the field's state. The name is HTML's term
  for a field that keeps Enter from submitting.
- **`rating::apply_event` stays on its scale.** It is the five-star
  fold; `apply_event_with(.., opts)` folds a `rating_with` scale.
  Star 0 is not a star, a star past the scale is not this rating's,
  and a value past the scale clears from the top star, where it is
  drawn. Chose-because: the fold cannot know the scale unless told,
  and the pair mirrors `rating` / `rating_with`. The cost: a ten-star
  rating folded with the plain `apply_event` ignores stars six to ten.
- **A key inside a virtual-list row resolves.** The key index is
  built before layout, when a virtual list has no rows, so
  `rect_of_key` — and a popover anchored to a row's button — found
  nothing in either kind of list. Realized rows add their keys as
  they are placed.

**Independent review of the three commits.** One reviewer, given the
diff and four areas to probe, not the reasoning above; each finding
re-run before it was acted on. Fixed in the follow-up commit:

- The aspect measure over-reserving in a column placed narrower than
  the room on offer (above). The same change settles an older case: a
  `Fill`-width aspect tile under a centring column reserved 200 px
  and was placed 0×0.
- Text selection and link hits under a scale resolved the byte with
  unscaled font metrics and unscaled padding, so every point past the
  first glyphs of a scaled paragraph read as the end of the text.
- Four of the five hit-test walkers had no test under an inherited
  scale, and the recorder half of the link-colour fix had none at
  all: both passed with the fix removed. Each now fails without it.
- The menu-row rule stopped at any keyed child, which took a keyed
  label or glyph out of the row; it stops at a painted child or a
  control.
- The section-body exemption looked one level down and missed a
  `Fill` canvas inside a row.
- `blocks_implicit_submission` on a node above the form was reset at
  the form, against its own doc.
- A fit-content row held narrower by its own `max_width` measured its
  fills against the wider room.
- `.ghost()` left a recipe's baked shadow (`card(..).ghost()`), which
  then appeared with the hover wash.

Found and left:

- **A stock `text_input` placeholder is now a `LowContrastText`
  finding in all four light palettes** (3.47–4.20:1), attributed to
  the app's call site. The reading is what is painted. It needed a
  palette ruling — see the table above — and got one on 2026-11-13
  ("Light palettes hold 4.5:1", at the end).
- **The disabled join cannot tell an authored opacity from the
  disabled fade** (both live in `El::opacity`), so an authored opacity
  on a disabled node under a disabled ancestor joins by `min` instead
  of multiplying: that node is drawn up to twice as strong as an
  enabled sibling with the same authored opacity. Opacities of 0 and 1
  are exact. Exact in general needs the two stored apart, which would
  cost the eased fade into the disabled look.
- A fit-content row measures each width-dependent `Fill` subtree
  twice, so such rows nested in one another's fills cost 2^depth
  (27 nodes, 8 deep: 3065 visits against 53). Rare and shallow in
  practice; unbounded.
- Older sizing cases the fixes do not reach: a `min_width` /
  `max_width` on a `Fill` child of a row (the share and the placed
  width part ways); an `Aspect` width under a row's stretched height;
  a column measured with no available width; a row placed at its
  content width with an *unset* `Hug` (centred, in a row, in an
  overlay).
- `hit_overflow` is not scaled with a scaled subtree; `UiTarget`'s
  `font_size` / `line_height` stay unscaled beside a scaled `rect`;
  the last few pixels of a link at the end of a paragraph do not hit
  at any scale.
- The dead-spacer exemption for a container at its own maximum also
  covers one that was never stretched; its content overflows either
  way and that is reported.

Not verified on a GPU or a device: every fix is checked at the draw-op,
layout or event level in `damascene-core`.

### Opacity for paint no colour carries (2026-11-12)

The owner, on the item the left list ended with: "Agreed with
implementing the pipeline changes for 1 before 0.9.0 -- you can
proceed."

**The rule.** A colour the library puts in a draw op is final: the
node's opacity is already in its alpha. Four ops draw paint no op
colour carries, and those carry the node's `opacity` beside it:
`AppTexture`, `Scene3D`, `Vector` (for a `Painted` asset) and `Icon`
(for an SVG icon in its authored colours). The recorder methods take
the same value, and it is for that paint only. Rejected: handing the
recorder unfaded colours plus one opacity for everything. It reads
more evenly, but a host that ignored the new argument would then
draw every icon in a faded subtree at full strength, where under
this rule it keeps what it draws today.

Two cases sit outside those four and follow the same idea by other
means. A quad bound to a shader of its own (a custom shader, and the
stock spinner, skeleton and indeterminate progress) gets its
uniforms as the widget wrote them plus the `opacity` instance
attribute, and the shader applies it. A colour glyph has no colour
argument of its own; it takes the alpha of the text colour it is set
in.

**The authored-colour icon was not on the list.** `SvgIcon::parse`
keeps an icon's own fills, and those went to the tessellator beside
a faded `currentColor`: a brand mark in a disabled button stayed at
full strength. Same fault as the painted vector, same fix.

**Where each is applied**

- **Painted vector, authored icon:** on the CPU, at tessellation.
  `VectorMeshOptions::authored_opacity` multiplies every fill, stroke
  and gradient the asset carries; `currentColor` takes the caller's
  colour as given. Gradient slots are keyed by opacity, so a faded
  copy of an asset gets its own. No vector shader changed.
  `VectorMeshOptions::painted(rect, opacity, space)` is the preset the
  three backends share (it replaced three copies of the white
  stand-in `currentColor`). Each path fades on its own, so paths that
  overlap show through one another, as a faded container's children
  do.
- **App texture:** one `f32` on the surface instance
  (`@location(4)`), multiplied into the premultiplied output of all
  three fragment entry points.
- **3D scene, and a plot's data layer:** the same shader composites
  the finished offscreen scene, so the scene fades as a group and
  overlapping marks do not show through. `CompositeInstance::new`
  takes the opacity; the struct is 44 bytes.

**`SurfaceAlpha::Opaque` now blends.** Its pipeline had a
(One, Zero) blend and could not fade. All three alpha modes share the
premultiplied blend now, and `fs_opaque` writes alpha = opacity: at
full opacity that is 1.0 and the destination is replaced as before.
Rejected: a fourth pipeline used only when faded, which keeps
(One, Zero) for the common case. The one difference it would buy is
a non-finite pixel already in an fp16 target, which (One, Zero)
overwrites and the blend does not.

**The SVG and the dump.** A scene's `<g>` gets a group `opacity`; an
asset's own paints get `fill-opacity` / `stroke-opacity`; the dump
lists ` opacity=0.50` on these ops below full strength and nothing at
full. The SVG had been dropping an asset's authored `fill-opacity`
and `stroke-opacity` altogether; it writes them now.

**Breaking for a custom host:** the four `TextRecorder` signatures;
the `opacity` field on `DrawOp::Icon`, `AppTexture`, `Vector` and
`Scene3D` (the enum is not `#[non_exhaustive]`); the public field
`VectorMeshOptions::authored_opacity`; `CompositeInstance::new`'s
arity and the struct's size (40 to 44 bytes); and the `@location(4)`
input of the stock `surface` shader.

**Checked on a GPU.** Each backend's headless render tests draw a
tile over a light backdrop at full opacity, at half, and not at all,
and read the frame back; summed over the frame, the half frame must
have moved half the way from the bare backdrop to the full one. The
cases: a solid-fill and a gradient-fill painted vector, an
authored-colour icon, an app texture in each alpha mode (the opaque
one also at full strength, where its centre must be the texture's
colour), a scene, a spinner, a skeleton, an indeterminate progress
bar and a colour emoji. Run on the workstation's Radeon through
wgpu, vulkano and ash. Each fix was then removed in turn, blend
states included: 38 removals, 38 caught. Not checked: WebGL2 in a
browser, Android, iOS, an fp16 target, or a windowed session.

**Independent review of the commit.** One reviewer, given the diff
and five areas to probe (paint that still ignores opacity, opacity
applied twice, instance layouts, the `Opaque` blend change, claims
against code). It found no fault in the four cases above on any
backend, nothing applied twice, and the layouts in agreement; it
rendered the changed shader through wgpu's GL backend as well. What
it found, each re-checked before it was acted on:

- **Spinner, skeleton and indeterminate progress ignored opacity**
  on all three backends (older than this work; this record had said
  otherwise). The three shaders declared the attribute and did not
  use it. They output straight alpha and now scale it.
- **Colour emoji ignored opacity** on all three backends (older).
  Each backend replaced the glyph's colour with opaque white for a
  colour glyph, dropping the alpha too. Chose: white at the text
  colour's alpha. Cost: an emoji in text with a translucent colour
  is now as translucent as the letters, where CSS would leave it
  opaque. Rejected: an op-level opacity on glyph runs, which is two
  more recorder signatures for a case the alpha already expresses.
- **The first tests could not see the blend state.** They drew over
  black, where "crossfades with what is under it" and "scaled, not
  blended" give the same pixel: the opaque pipeline put back to
  (One, Zero) passed all of them. The claim "each test fails with
  its fix removed" in the commit message was therefore false for
  that fix. The tests draw over a light backdrop now.
- **The lane plot's scene op had no test**; the core draw-op test
  covers it.
- **The SVG had started writing an authored `fill-opacity` for a
  one-colour icon**, which the GPU draws from coverage alone. It
  writes none for those again.

Left:

- The draw-op dump prints a token colour by its name and drops its
  alpha, so a faded one-colour icon in a token colour reads the same
  as an unfaded one. True of every token colour in the dump, and
  older than this work.
- A filled built-in icon under the `Relief` or `Glass` icon material
  is tessellated as a fill and a stroke that overlap; faded to half
  it reads 0.56 (the reviewer's measurement, not re-run). The flat
  material draws it from one coverage mask and reads 0.50.
- The fixture glass shaders (`liquid_glass*.wgsl`) do not read the
  opacity attribute, so a glass panel in a faded subtree stays at
  full strength. By reading only.
- `skeleton_with_color` is not re-exported beside `skeleton`, where
  `spinner_with_color` and `progress_indeterminate_with_color` are.
- The SVG stand-in `currentColor` for a painted `vector()` is the
  foreground token at 1.5 units; the GPU's is white at 1.0. Older
  than this work.
- The three backends each define a surface instance struct with the
  layout of core's `CompositeInstance`.

### Light palettes hold 4.5:1 (2026-11-13)

The owner, on a three-part proposal (the trough, three token values,
the gate): "Agreed, let's apply 1, 2, 3". Assent to the proposal as
written, without further reasoning of his own.

**What was measured.** Every showcase scene, desktop and phone, with
the two open-menu scenes, linted under each stock palette. The four
dark palettes were clean. The light ones were not (the table in "The
review's left list" counted the same sweep without the open-menu
scenes: 34, 44, 44, 12):

| Palette | Findings | What |
|---|---|---|
| damascene-light | 42 | `muted-foreground` on `muted` / `accent` 4.40:1 (40); on the trough 3.47:1 (2) |
| radix-slate-blue-light | 46 | `accent-foreground` on `accent` 4.25:1 (34); four text tokens on the trough 3.31–4.13:1 (12) |
| radix-sand-amber-light | 46 | `accent-foreground` on `accent` 4.25:1 (34); four text tokens on the trough 3.20–4.20:1 (12) |
| radix-mauve-violet-light | 12 | four text tokens on the trough 3.59–4.28:1 |

Two different faults. The trough was ours. The token pairs are the
ladders' own values, a tenth to a quarter short of the floor.

**1. In a light palette the trough is `muted` itself.**
`Palette::sunken_fill()` is the fill `SurfaceRole::Sunken` and
`SurfaceRole::Input` paint: `muted` darkened 8% in a dark palette, as
before, and `muted` in a light one. A palette is light when its
`background` is lighter than its `foreground`, so a custom palette
gets the same rule. Chose-because: the 8% is an OkLab step towards
black, which is small on a dark colour and large on a light one —
zinc-100 `#F4F4F5` came out at `#DADADB`, darker than the trough's
own border (`#E4E4E7`) — and no text token held 4.5:1 on it.
Rejected:

- *A smaller darkening in light.* `warning-tint-foreground` is
  4.54:1 on `muted` in the three Radix palettes; any step darker
  than `muted` puts it under the floor and widens the token changes.
- *No trough in light* (shadcn's light input is transparent with a
  border). A larger change of look, and `Sunken` would paint nothing.
- *A palette slot for the trough.* A new public field on a struct
  apps build, for a value nothing has asked to set.

**2. Three text values move.**

| Palette | Token | Was | Is | On `accent` | On `muted` |
|---|---|---|---|---|---|
| damascene-light | `muted-foreground` | `#71717A` zinc-500 | `#6E6E77` | 4.40 → 4.59 | 4.40 → 4.59 |
| radix-slate-blue-light | `accent-foreground`, `link-foreground` | `#0D74CE` blue-11 | `#0C6CBE` | 4.25 → 4.80 | 4.19 → 4.73 |
| radix-sand-amber-light | `accent-foreground`, `link-foreground` | `#AB6400` amber-11 | `#A05D00` | 4.25 → 4.76 | 4.05 → 4.54 |

The two Radix targets are the values those palettes already carry as
`info-tint-foreground` and `warning-tint-foreground`, tuned on
2026-08-12 for the same reason. This reopens the zinc-ladder ruling
of 2026-10-14 for one value: `#6E6E77` is three levels a channel
under zinc-500 and is not a Tailwind step. Rejected:

- *Zinc-600* (`#52525B`, on the ladder): 7.0:1, and muted text then
  reads nearly as strong as body text.
- *Excusing the designed pairs*, as the white primary label is
  excused. That exception keeps a brand colour the oracles ship; this
  would keep a difference no eye sees and have the lint pass text
  under the floor.

Believed and not checked against their sources: shadcn and Radix
Themes ship the original pairs as they were.

**3. The gate lints every stock palette.**
`dump_showcase_bundles` writes its artifacts under the theme the
showcase opens in, as before, then builds and lints each scene under
the other seven; a finding under any fails the run.
`text_tokens_keep_aa_on_every_neutral_surface` (palette tests) holds
eleven text tokens to 4.5:1 on `background`, `card`, `popover`,
`secondary`, `muted`, `accent` and the trough, in all eight palettes.

**Checked.** The sweep reads 0 in all eight palettes. Each of the six
changes (the trough rule and five token values) was removed in turn:
the unit test fails for all six; the gate fails for four — the
showcase sets no link on a `muted` or `accent` surface, so the two
`link-foreground` values are held by the unit test alone. The dark
artifacts are byte-identical apart from shifted source line numbers
in the tree dumps. Text inputs, forms, typography and buttons were
rendered on the GPU (wgpu) under three light themes before and after,
and looked at.

**With it.** `damascene-workbench` copied the trough derivation in
three places "so the key and the rendered control cannot drift"; it
calls `Palette::sunken_fill()`. `render_showcase_sections` takes a
theme token (`-- damascene-light`), which is how the light pages
were rendered.

Left:

- In a light palette the trough now equals `muted`, and in all four
  `secondary` is the same colour. A `muted`- or `secondary`-filled
  child inside a trough — a secondary button in an `input_group`, a
  selected row in a sunken band — is told apart by its border alone,
  where it has one. No stock widget nests them.
- `.primary()` on a menu row measures 1.58:1 on one light palette.
  Recorded here as "a filled row, not a text token", which was wrong
  on both counts: it is the label's colour, and the fault is the
  `primary` fill used as text in every palette that makes it a brand
  colour. Closed in the next section.
- The contrast floor is WCAG 2's. Nothing here measures the border
  of a control against the page (1.4.11).

### `.primary()` text takes a text-grade token (2026-11-13)

The owner's words on the proposal below: "Agreed. The
bottom-navigation icon can move to the new token. Proceed." — assent
with one decision of his own (the icon).

**The fault.** `.primary()` colours text on six shapes — plain text,
a ghost button, an outline button, a boxed badge, a tinted card, a
menu row — and all six used the `primary` token. The comment on the
modifier said `primary` "is a neutral (near-white / near-black),
already text-grade". True of the two zinc palettes; in the six Radix
ones `primary` is step 9 of the brand scale, a solid made to sit
under a label. Worst measured ratio of `primary` as text:

| Palette | Plain surface | On its own 15% tint (page, card) |
|---|---|---|
| damascene-dark | 14.27 | 5.02 |
| damascene-light | 16.12 | 15.23 |
| radix-slate-blue-dark | 4.56 | **3.26** |
| radix-slate-blue-light | **2.87** | **2.86** |
| radix-sand-amber-dark | 9.96 | **4.44** |
| radix-sand-amber-light | **1.39** | **1.47** |
| radix-mauve-violet-dark | **2.84** | **2.43** |
| radix-mauve-violet-light | 4.73 | 4.63 |

"Plain surface" is the worst of page, card, popover, `secondary`,
`muted`, `accent` and the trough. The lint reported all of it when
asked; the gate did not ask, because the showcase put `.primary()`
on solid buttons only. The menu row came to notice on 2026-11-11,
when a status on a menu row stopped filling the row and began
colouring its label; `.ghost().primary()` had the fault before that.

**The fix.** `primary` gets what the four statuses got on 2026-08-12:
a text-grade counterpart, `tokens::PRIMARY_TINT_FOREGROUND` backed by
`Palette::primary_tint_foreground`. The zinc palettes hold `primary`
in it, so nothing moves there. (Since the pre-release review, last
section, the slot is `Option<Color>` and the zinc palettes hold
`None`, which means `primary`: a custom palette built on one keeps
its own primary as `.primary()` text.) The six Radix palettes hold step 11 of
the brand scale — the value `accent-foreground` already carries in
each, which in the two light ones that needed it is step 11 taken a
shade darker (previous section). This is Radix's own split: step 9
fills, step 11 text. `.primary()` passes it wherever it colours text,
and a boxed chip pairs the `primary` tint with it; a solid keeps
`primary` and `primary-foreground`.

| Palette | Value | Plain surface | On the tint (page, card, popover) |
|---|---|---|---|
| radix-slate-blue-dark | `#70B8FF` blue-11 | 7.08 | 5.06 |
| radix-slate-blue-light | `#0C6CBE` | 4.73 | 4.71 |
| radix-sand-amber-dark | `#FFCA16` amber-11 | 10.26 | 4.57 |
| radix-sand-amber-light | `#A05D00` | 4.54 | 4.81 |
| radix-mauve-violet-dark | `#BAA7FF` violet-11 | 7.34 | 6.28 |
| radix-mauve-violet-light | `#6550B9` violet-11 | 5.41 | 5.30 |

Rejected: *reusing `accent-foreground`* with no new token. The value
is the same today, but a custom palette may set that token to
something unrelated to its brand colour (shadcn's coloured themes
keep it neutral), and a tree dump would name `accent-foreground` on
a `.primary()` node.

**The bottom-navigation glyph** of the current item takes the new
token too (the owner's call). It was `primary`: on the light sand
strip the current glyph was pale yellow at 1.58:1 beside dark grey
muted ones, the faintest thing in the bar. The lint does not measure
glyphs, so nothing reported it. The caption stays `foreground`.

**Fences.** `text_tokens_keep_aa_on_every_neutral_surface` holds the
new token with the other eleven; `primary_text_keeps_aa_on_its_own_
tint` holds it on the 15% tint over page, card and popover;
`tinted_status_text_meets_aa_in_every_palette` lints six primary
shapes with the status ones under all eight themes; the showcase's
Palette page carries a ghost, an outline, a chip and a line of
primary text, which puts the path under the gate. `LowContrastText`
names `tokens::PRIMARY_TINT_FOREGROUND` when `primary` itself is the
failing text colour, as it names the status tokens.

**Checked.** Each change was removed in turn — the modifier's text
token, the chip pairing, the glyph, the lint hint, and the six Radix
values set back to step 9. Nine of the ten fail a unit test; the gate
fails for seven (it cannot see the glyph or the hint). The tenth,
violet-light at violet-9, fails nothing, because violet-9 passes
there. The Palette page was rendered on the GPU (wgpu) under four
Radix themes before and after, and a bottom-navigation bar under
three; on the rendered pixels the new token measures 4.66:1 at worst
(the light sand outline button, whose shadow greys its unfilled
interior to `#F3F3F3`).

**With it.** The showcase's Palette page says it shows "every
semantic token" and listed none of the text-grade ones; it and the
`palette_demo` example now list all five (`primary-`, `destructive-`,
`success-`, `warning-`, `info-tint-foreground`). The README's
`assets/showcase_palette.png` is that page and was rendered again.
`damascene_dark_matches_token_fallbacks` had skipped the four status
`*_TINT_FOREGROUND` constants; they are in its list. The widget kit's
colour vocabulary names the text-grade tokens.

Left:

- **Seen in those renders, older and not part of this change: a
  status-tinted alert in a light theme is a muddy slab.** The alert
  is a `Panel`, the panel's drop shadow is drawn under the whole
  silhouette, and the 15% tint lets it through: the info alert on a
  white card renders `#C1C8D8` in radix-slate-blue-light, and
  `#EDF3FF` — the tint alone — once it is given `.shadow(0.0)`
  (damascene-light: `#C2C4D4` and `#EEF0FC`; damascene-dark moves by
  four levels and shows nothing). Its text measures **3.21:1** on
  the rendered pixels (3.87 in damascene-light) where the lint,
  which composites the tint over the card, computes 4.8. The
  section "A fading surface takes its shadow with it" kept this on
  purpose ("a fill that is translucent by design still sits on its
  darkened ground"). The previous section's "light palettes hold
  4.5:1" does not hold for these alerts as drawn. Closed in the
  next section.
- Thin primary indicators keep the fill: the tab underline, the
  active one-time-code cell's border, an outline button's border,
  the checked checkbox, the switch track, the slider and progress
  fill. On the light sand palette they are 1.4–1.6:1 against the
  page, which is what Radix ships; this is the border-contrast item
  (WCAG 1.4.11) the previous section left.
- A boxed `.primary()` chip set on `muted`, `secondary`, `accent` or
  the trough, rather than on the page or a card, is 4.27–4.42:1 in
  three palettes (slate-blue-light, sand-amber-dark and -light). The
  status chips are pinned on page and card only as well and were not
  measured on the others.
- The violet-light value is step 11 for consistency; violet-9 would
  pass there (4.63 at worst), so no contrast test holds that one
  value.
- The fixtures colour demo icons with fill tokens (`primary` and the
  statuses alike) in the icon gallery and the media page. Glyphs,
  not text, and not changed.
- The `palette_demo` example prints about 300 lint findings of its
  own (a hand-rolled chip the `ReinventedWidget` lint fires on, an
  unnamed search field). Older; nothing gates the example.

### A shadow is cast outside its surface (2026-11-13)

The owner, on the recommendation to follow CSS: "This looks good.
Let's apply the recommended fix to the alert shadows."

**The fault.** `stock::rounded_rect` painted a node's drop shadow
under its whole silhouette and composited the fill over it. An
opaque fill hid the shadow. A fill that is translucent by design did
not:

| As drawn, light themes | Before | After |
|---|---|---|
| Info alert, radix-slate-blue-light | `#C1C8D8`, text 3.21:1 | `#EDF3FF`, text 4.83:1 |
| Info alert, damascene-light | `#C2C4D4`, text 3.87:1 | `#EEF0FC`, text 5.91:1 |
| Outline button's interior on a white card | `#F3F3F3` | `#FFFFFF` |

All four status alerts were muddy slabs in a light theme, and the
contrast lint — which composites the tint over the card and knows of
no shadow — passed text that was under the floor on the pixels. Dark
themes moved by a few levels and showed nothing. The shader and the
alert recipe are the same at the 0.8.0 tag (read, not rendered).

**The rule.** A shadow is CSS `box-shadow`: cast outside the
silhouette and never drawn under it, whatever the fill's alpha. In
the shader the shadow is `shadow × opacity × (1 − inside)`, and the
fill and the shadow *add* — on an antialiased edge the surface covers
`inside` of the pixel and the shadow the rest, and the two share no
area.

This reverses a choice of 2026-11-11 ("A fading surface takes its
shadow with it"), which kept the shadow under a translucent fill so
that nothing that was not fading changed. Rejected now because it is
not what a browser draws, it turned every status alert in a light
theme grey, and it made the lint's measurement untrue of the pixels.
Also rejected:

- *Taking the shadow off the alert only.* One widget; a tinted card,
  an outline button and any app surface with a translucent fill keep
  the fault, and the lint stays wrong about all of them.
- *Masking the shadow and then compositing the fill over it*, the
  obvious code. On a half-covered edge row it leaves a quarter of
  the halo where there should be half: measured 8 against 15 of 255
  under `SHADOW_MD`, a light seam around every shadowed surface,
  opaque ones included.

**What does not change.** An opaque surface: the sum is, for an
opaque fill, the value fill-over-shadow gave, at every opacity (edge
row 15 before and after). Of the showcase's 22 pages rendered under
four themes, 18 are pixel-identical; the four that differ are the
ones with status alerts and outline buttons.

**What does.** Status alerts and status-tinted cards; the interior of
an outline button; and, for an app, any node with a shadow and a
translucent or absent fill — it now shows what is behind it, where
it showed a ground darkened by its own shadow.

**SVG export** mirrors it. Each shadow filter casts from the
*saturated* source alpha, so a 15% tint casts a full shadow (it cast
15% of one), and `feComposite operator="out"` cuts the shadow from
under the shape. Checked in resvg and rsvg-convert. One limit: a
stroked shape with no fill is only its outline in SVG and casts the
outline's shadow, inside as well as outside. (Replaced the same day,
last section: the saturated cut was one-bit and left a light seam on
antialiased edges. The shadow is now a separate element, and the
outline limit is gone.)

**Checked.** Readback tests on all three backends
(`a_shadow_is_cast_outside_its_surface_and_never_under_it`: wgpu in
`shadow_render.rs`, ash and vulkano in `scene3d_render.rs`): a
half-transparent fill and a fill-less outline show the page, not the
shadow, and cast the halo an opaque surface casts. With the old
shader all three fail. The wgpu test also pins the half-covered edge
row, which the mask-then-over variant fails, and
`a_status_alert_in_a_light_theme_is_its_tint_not_a_slab` renders the
alert under damascene-light and holds its text to 4.5:1 on the drawn
pixel.

Left:

- A custom shader draws its own shadow, or none; nothing here
  reaches it.
- The lint's model (tint over the surface behind) now matches the
  pixels for the stock surfaces. Nothing compares the two in general:
  the alert test is the one place a lint figure is checked against a
  render.

### Pre-release review (2026-11-13)

Owner, asking for it: "Let's run one more review pass for any other
things that should hold up 0.9.0." And on the report, which put four
decisions and four fixes to him: "Proceed with the recommended fixes
on 1 through 4, as well as the "Fixes that need only your go" items."
That is assent to the recommendations as written, without reasoning
of his own: a checkpoint, cheap to reopen.

**How it was run.** Four reviewers with no context from the work,
each given the code and one question: the two changes of this
morning that had had no independent read (the two sections above);
the public API that is new since 0.8.0 and would be breaking to
change later; the shipped docs against the code; the release-notes
draft against the code at both ends. Beside them, the release
instruments: `cargo semver-checks` for the twelve published crates
(matches the notes' table), a trial build of two apps that are on
0.8.0, the web showcase in headless Chromium on WebGL2, the package
file lists, and the README images against fresh renders. The
reviewers' leading findings were checked against the source before
anything was built on them.

**1. The scrollbar gutter can be refused.** The trial build of a
phone app is what found it. Its setup wizard sizes a picture from
the page's full width and was written to have no gutter; the default
rule (18216b8, ruled 2026-10-16) gave it one, the picture overflowed
by 12 px, a label was clipped, and two of the app's own tests failed.
The rule had no off switch short of `.no_scrollbar()`.
`El::scrollbar_gutter` is `ScrollbarGutter { Auto, Stable, Never }`;
`.no_scrollbar_gutter()` sets `Never`. The default rule is unchanged.
Rejected: no gutter on touch hosts (the same app reserves the gutter
by hand on its other pages, so a phone is not a reason by itself).

**2. The primary text slot is optional.** This reopens part of the
section two above, with evidence: both the code reviewer and the API
reviewer reported that a custom palette which assigns `primary` and
not the new slot silently keeps the stock palette's text colour, and
three palettes in this repository had done exactly that.
`Palette::primary_tint_foreground` is `Option<Color>`; `None` is
"the same as `primary`", held by the two zinc palettes. A custom
palette on a zinc base keeps its own primary as `.primary()` text, as
in 0.8.0, and `LowContrastText` names the slot when that is too weak.
A custom palette on a Radix base still inherits that base's text
step: the trap is narrower, not gone. Rejected: a lint for "primary
differs from the base" (a palette does not know its base).

**3. API consistency for what 0.9.0 adds.** `sidebar_with(opts,
children)`; `resizable_panel_group(key, axis, children)`;
`data_row(key, id, ..)` and `data_expander(key, id, ..)` mint their
routed keys; `swatch` for `color_swatch`; `SectionOpts::actions` for
its list-taking `trailing`; the data table's generic names leave the
prelude; `DataColumn`, `data_table::Sort`, `DataTableState`,
`DataTableAction`, `TableColumn` and `SidebarVariant` are
`#[non_exhaustive]`. Each is in the commit message with its reason.
Left alone on purpose, and still open as API questions: the
`apply_event` argument order of `numeric_input`, `number_scrubber`
and `input_otp` (older, would break 0.8 users for consistency only);
`EditorTabsConfig`'s name; `El::filled()` against the `icon_` prefix;
`rating_display(u8, u8)`, which cannot show 3.5; sort state by column
index; the two meanings of the bool `apply_event` returns ("handled"
on some widgets, "changed" on others); `HtmlOptions` and
`MarkdownOptions` have no `const` construction.

**4. A scroll anchor is kept only for the node it was recorded on.**
Seen first in the browser check: switching the showcase from About
to Palette opened Palette 200 px down. Scroll anchoring names its
element by computed id, which for an unkeyed node is its position, so
after a page swap it "kept" an unrelated node in place. The anchor
now carries an identity and is dropped when the node at its id is a
different one: the node's kind, the start of the text it shows and,
unkeyed, the source line that built it. The showcase also keys its
content pane by section, so pages no longer share one stored offset.
The mechanism is from May; 0.8.0 was not run.

The first form of that identity (5e1ac35) folded the source line of
every node on the path from the scrollable down. The review of this
batch (below) showed it wrong both ways. It dropped the anchor of one
page across a breakpoint, where the wrappers are built by the other
arm of an `if` at the moment the anchor is needed (measured: the
reading point moved 206 px where it had held). And it kept the anchor
across a swap between two pages built by the same lines, a detail view
showing another record. Rejected with it: folding only the anchored
node's source line (two records from one builder still match). What
the reader was looking at is what it says, so the text is the
identity, and the node's own line stands in where there is no text.
Still not told apart: a textless node (a picture, a plot) built by the
same line on both sides of a swap. Keying per page is the rule that
covers every case, and `scroll()`'s docs now say so.

**The fixes.**

- *Docs.* The reviewer compiled every code block in the shipped
  READMEs, the widget guide and the widget module docs: 5 blocks and
  11 inline call shapes did not compile and about 25 statements were
  false or stale, many of them older than 0.8.0. All corrected; the
  shapes this pass added or changed were compiled in a scratch crate.
  Two claims were wrong in substance and are now stated as they are:
  the library does read three `Kind`s (`Modal` traps focus, `Scrim`
  takes no hover, `Heading` is selectable), and a forked widget's
  authored values are exempt from the theme scales where a stock
  widget's defaults are not. The code was not changed to match the
  old claims. Added: a "Theme and palette" section in the core README
  (where a theme goes, the custom-palette recipe, built options),
  table rows for the widgets new in this release, per-side borders.
- *SVG export.* The shadow is a separate element (see the note in the
  section above); the seam is gone in resvg and rsvg-convert.
- *Invalid text.* `text(..).invalid()` takes
  `DESTRUCTIVE_TINT_FOREGROUND`; it was the fill-grade red, under
  4.5:1 in all eight palettes.

**Checked.** Each code change has a test that fails without it (the
anchor test reads 520 px without the identity check; the gutter,
palette and invalid tests are new). Full local gates on each pushed
head. The trial builds were run again on the result: whisper-agent
builds with no source change, and the phone app, with its two compile
fixes and `.no_scrollbar_gutter()` on the wizard's scroll, passes its
own 171 tests. A rebuilt web showcase on WebGL2 opens every page at
its top.

**Reviewed.** One more reviewer read the batch itself. Found and
fixed: the anchor identity (above); a toast's dismiss button could be
focused and pressed from the keyboard or a screen reader and nothing
happened (older; it matters most under a screen reader, where toasts
do not time out) — the runtime now consumes that activation as it
consumed the click; `data_row` / `data_expander` took `&impl Display`
and so rejected a string literal, where `tab_trigger` takes a value;
the new lint hint had lost its line continuations and printed runs of
spaces; and the corrected docs still overstated two things, which now
read as the code is: library code reads more kinds than three
(`sheet_content` and `popover_layer` for focus layers, `item` /
`item_rail` / `status_dot` in the style modifiers), and stock widgets
share a few crate-private helpers besides the `default_*` setters.

Left, beyond the API questions under 3:

- Thin `primary` marks still take the fill: the selected radio dot
  (1.58:1 on a light sand card, and it is the only cue) and the
  spinner arc belong with the list in the `.primary()` section. No
  lint measures a glyph or a mark.
- When the line a quarter of the way down a scrollable falls in the
  gap between two blocks, the anchor is their container, and the
  offset absorbs only part of a change above it (measured by the
  reviewer: a 100 px growth moved the reader's line 47 px). Older.
- `field_with(label, control, opts)` keeps its options last. Its
  content is one control, not a list, so it follows the leaf
  constructors (`text_input_with`, `meter_with`); the container rule
  is "the children list is last".
- `ScrollbarGutter` is reachable as `tree::ScrollbarGutter`, not from
  the prelude; the two modifiers are the intended way in.
- `default_*` setters are crate-internal, so a forked widget cannot
  mark a value as a theme default. The docs now say so.
- The SVG halo is wider near the edge than the GPU's (the recipes'
  negative spread is not mirrored).
- WebGPU in a browser, a real window and a phone were not run for
  any of this. WebGL2 in headless Chromium was: the light-theme
  alerts render at the native value and the 3D scene composites.
- The advisory scan was not run (the installed `cargo-audit` cannot
  read the current database).


### Released as 0.9.0 (2026-11-13)

The owner, on the report of the pre-release fixes: "Good work. Let's
proceed with the prepare commit and release process."

`v0.9.0` is on `278ad7c`: the prepare commit (`423b4b9`) and one
lockfile commit after it. All twelve crates are on crates.io at 0.9.0
and docs.rs built every one of them, the platform crates for their
own targets. The release notes are the GitHub release; they are the
"breaking list" this record kept adding to, checked against the code.

Checked on the release commit: the local gates (2440 tests) and CI on
both remotes; `cargo package` on the twelve crates together, which
builds each from its tarball against the others; the README images,
re-rendered and byte-identical; the web showcase rebuilt and driven in
headless Chromium on WebGL2 (a page opens at its top, and each page
keeps its own scroll offset). The packaging run is what found the two
yanked lockfile entries; it is step 2 of `docs/RELEASING.md` now, and
the publish order is `tools/publish_crates.sh`.

Shipped without a run of the last two days' changes on WebGPU in a
browser, in a real window or on a phone, and without an advisory
scan. The "Left" lists above are the open work; the release went
ahead with them on the table.
