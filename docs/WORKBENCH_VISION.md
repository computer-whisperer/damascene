# Damascene Workbench — Design Record

Maintainer-facing. Ratified 2026-07-25. This records the emulation-target
decision for the dense-application opinion crate, the alternatives rejected
and why, and the calibration plan — before any code exists, so the arc can
run at full force without silently relitigating the premise.

## The decision

A new opinion crate — working name `damascene-workbench` — whose emulation
target is **the VS Code workbench**: its theme-token vocabulary, its
density metrics, and its layered surface model. shadcn remains the
vocabulary for everything the workbench does not opine on: forms, dialogs,
buttons, and controls keep their shadcn anatomy, resized and squared to
workbench metrics. Blender remains an *interaction* reference at the app
layer (split trees, per-pane tool strips) and contributes nothing to the
skin.

Stock damascene stays shadcn. This crate exists because shadcn is a
landing-page and dashboard vocabulary, and three real applications built
carefully on the stock defaults — eutectic (ECAD workbench), rumble
(Mumble-clone voice client), webslicer (printer slicer) — consistently
read as demos rather than tools. The purpose of the crate is to close
that gap without inventing vocabulary agents don't already know.

## The diagnosis — why stock apps read as toys

Surveyed 2026-07-25 across all three apps: none of them fights the stock
theme (zero `.radius(0)` overrides, no density hacks, no hand-rolled
hairline layers beyond two in eutectic). The dissatisfaction is not a
missing-vocabulary problem; the apps are written cleanly against stock
vocabulary and simply *look like shadcn*. Four structural signals, all
deliberately maximized by landing-page design and deliberately minimized
by professional tools:

1. **Control scale.** shadcn Md is 36px controls with 14px type and
   generous padding; tools run 22–28px rows and 11–13px type. Oversized
   controls are the strongest single "demo, not tool" signal.
2. **Radius and shadow.** 8px corners and soft shadows say marketing
   card; tools are 0–4px and hairlines.
3. **Flat surface model.** shadcn dark is one background with outlined
   cards floating on it; tools are *layered* — sidebar darker than
   editor, bars darker still, regions separated by 1px borders, nothing
   floating.
4. **Whitespace as luxury.** Landing pages spend pixels to look calm;
   tools spend pixels on information. `SPACE_3` gaps between workbench
   regions read as separate widgets on a canvas, not one instrument.

A workbench profile must invert all four. All four are values-and-recipes
changes, which is why an opinion crate is the right mechanism.

## What "emulating VS Code" means, precisely

Three binding layers:

**1. Token vocabulary.** The crate's palette tokens are derived from VS
Code theme keys — `side_bar`, `status_bar`, `panel`, `editor_group`,
`title_bar`, `list.active_selection`, `badge`, `focus_border` — minted
through the palette token namespace. This is the most heavily
corpus-represented dense-app token system in existence: hundreds of
thousands of published theme files name these keys, and the product
itself is the most-used developer tool on earth. An agent told "make it
look like VS Code" has deeper priors here than for any other tool
aesthetic, which is what damascene's acceptance test demands.

**2. Density metrics.** Row heights, type scale, control heights, and
radii measured from the real product (indicatively: ~22px list rows,
~35px tabs, 13px UI type, 0–5px radii — exact values are established by
calibration, not by memory; see below).

**3. Layered surface model.** Fills-and-hairlines elevation: each
workbench region is a distinct surface level separated by 1px borders.
No floating cards, no shadows for chrome. Popovers/menus keep a small
shadow and radius, matching the product.

Explicitly **not** binding: VS Code's DOM, its widget internals, its
exact widget shapes where shadcn already has an anatomy (a select is
still a shadcn select, at workbench size and radius).

## Rejected alternatives

Recorded with their real corpses, per house rule.

- **IBM Carbon.** Real spec, dense-capable, and the subject of the
  `damascene-carbon` experiment. Rejected because: corpus-thin relative
  to the winner; foreign token names (`layer-01`, `interactive-01`) that
  agents do not write unprompted, violating the acceptance test; and an
  IBM-blue enterprise identity none of the three consumer apps wants.
  The experiment did its job — it proved out-of-tree opinion crates work
  with zero core changes and produced transferable widgets.
- **Blender (as skin).** Right interaction model for eutectic — and the
  ui-oracle already adopted it — but its widget styling has essentially
  no corpus presence *as code*. Kept as interaction reference at the app
  layer; rejected as theme target.
- **Linear / "Tailwind-zinc devtool" genre.** Closest to the desired
  pixels (the eutectic ui-oracle's values are literally Tailwind zinc +
  blue-500), and agents write the genre fluently. Rejected as the named
  target because it is a genre, not a system: no spec, no measurable
  ground truth, no token vocabulary. "Choose and stick with it" needs
  something to stick to; you cannot screenshot-calibrate against a vibe.
  The genre's pixels survive anyway — VS Code-class tools are the
  genre's ancestor, and the oracle's zinc values map cleanly onto VS
  Code keys.
- **Ant Design / GitHub Primer / Fluent.** Dense-capable web-app
  systems, but CRUD-admin shaped rather than workbench shaped, and each
  is a weaker corpus citizen than the winner for the "make it look like
  a tool" prompt.

## Consumers and rollout order

1. **eutectic** — first consumer. Only app with a written, binding
   aesthetic spec (`docs/ui-oracle/` in the ecad repo); ~9.5k lines of
   workbench GUI whose region inventory is already VS Code's
   (menu bar, icon toolbar, split editor area, accordion side rail,
   status bar, command palette). Its existing chrome is the crate's
   acceptance test.
2. **rumble** — sidebar-tree + main + status bar, the same skeleton, and
   the deepest stock-anatomy consumer (69 card/sidebar/dialog sites). It
   already mints semantic tokens (`rumble-talking`, …) through the
   palette passthrough; those ride on top of the workbench palette
   unchanged.
3. **webslicer** — last. The slicer page (viewport + settings rail) fits;
   the queue/history pages are genuinely dashboard-shaped and may stay
   closer to stock.

## Relationship to the eutectic ui-oracle

Division of ownership: the **oracle owns anatomy and interaction law**
for eutectic (region inventory, view kinds, tool-memory semantics,
split-tree behavior); the **workbench crate owns the skin** (palette,
metrics, chrome recipes). The oracle's color table already carries a
"Damascene theme role" column — the crate is that column's landing
place. Illustrative mapping (non-binding until calibration):

| oracle | VS Code key family |
|---|---|
| bg-0 app bg | `editor.background` |
| bg-1 sidebar | `sideBar.background` |
| bg-2 bars | `statusBar.background` / `titleBar.activeBackground` |
| bg-3 headers | `sideBarSectionHeader.background` |
| bg-4 popover | `quickInput.background` / `dropdown.background` |
| bg-5 chip | `badge.background` |
| border-1/2/3 | `sideBar.border` / `input.border` / popover borders |
| accent | `focusBorder` / `list.activeSelectionBackground` / `button.background` |

Where the oracle's zinc values and stock VS Code Dark+ values disagree,
the oracle wins for eutectic — it is, after all, a *theme* of the
workbench vocabulary, exactly as a VS Code color theme is.

## Calibration plan

Same discipline as `POLISH_CALIBRATION.md`, new reference target:

- `references/vscode-calibration/` — sibling of
  `references/shadcn-calibration/`: screenshots of the real product at
  known scale (list rows, tabs, status bar, quick pick, settings
  editor, context menus), plus the official theme-key reference and one
  or two exhaustive published themes as token ground truth.
- Metrics (row heights, type sizes, paddings, radii) are **measured from
  screenshots**, never recalled. Indicative values in this document are
  hypotheses until then.
- One caveat weighed at ratification: VS Code has no published component
  library — its anatomy is a product, not an npm package. Component
  recipes are therefore calibrated from screenshots and theme keys
  rather than copied from reference source as shadcn's were. Acceptable;
  the screenshot-diff method doesn't care — but it makes the reference
  directory more load-bearing than it was for shadcn.

## Relationship to the core proposals (`VOCABULARY_PARITY.md`)

The workbench crate is a second, non-Carbon forcing function for the
parity proposals, and re-ranks them on stronger evidence:

- **Per-side borders** — unchanged, land first. Workbench chrome is
  hairline-separated everywhere (menu bar under-rule, status bar
  over-rule, pane headers, accordion sections).
- **Open token namespace** — gains its second consumer, this time not
  Carbon: the workbench key set (`side_bar`, `status_bar`, `panel`, …)
  plus rumble's semantic tokens. Moves from "honestly Carbon-motivated"
  to genuinely evidenced.
- **Radius scale (multiplicative)** — its defer-condition ("wait for a
  second consumer") is now met: the workbench look is the shadcn ladder
  scaled down to ~0–5px, which is exactly what the knob expresses.

## damascene-carbon disposition

The experiment retires. Transferables migrate into the workbench crate
as they're needed: the `data_table` skeleton (column specs shared
between header and virtualized rows, numeric alignment, selection),
`hairline()` (until per-side borders land in core), and the
layered-surface concept (Carbon's `layer-01` ramp is structurally the
workbench surface ramp). Once migration is complete the crate is
deleted; this document is its record.

## First-cut scope

- Palette (VS Code key set, oracle values as the default dark theme) +
  dense metrics profile.
- The chrome widgets core lacks: status bar, chip row, pane header.
  Core already has `menubar`, `toolbar`, `editor_tabs`,
  `resize_handle`, `number_scrubber`.
- Consume eutectic's existing chrome as the test — success is eutectic's
  `viewer_body` on the workbench theme matching the oracle, with
  app-side changes confined to deleting local styling workarounds.

Non-goals for the first cut, deferred deliberately:

- Hoisting eutectic's Blender-style split-tree machinery into a shared
  crate. It stays app-side until a second app needs it.
- A stock tree view. All three apps need one (rumble's room tree, the
  slicer settings tree, eutectic's explorer), but rumble's is the only
  implementation and it is deeply domain-entangled; hoisting is its own
  design, on tree-view terms, later.
- `data_table` in core. Same rule: hoist the style-neutral skeleton when
  the second consumer (eutectic's netlists/DRC tables) actually lands.
  **Lifted 2026-07-28 — see below.**

## Open questions

- Rust constant naming for the token set: `STATUS_BAR_BG`-style flat
  constants (matching `tokens::` house style) vs. structured
  `workbench::status_bar::BG`. Cosmetic, decide at implementation.
- Whether the crate ships light themes at all, or dark-only until a
  consumer asks.

## Retarget ratified 2026-07-27: values move to zinc/slate-blue; Dark Modern demotes to a variant

The two validation rounds (`references/workbench-validation/`, blind
pairs then real-shadcn round 2) plus the naming-oracle work
(`docs/NAMING_ORACLE.md`) settled a question this document originally
conflated: the crate's payload was always four things — VS Code **key
vocabulary**, Dark Modern **values**, the **density profile**, and the
**chrome recipes** — and only the values were ever VS-Code-specific.
The toy diagnosis's four signals never included color.

**Ratified:** the workbench default paints from the stock
`radix_slate_blue_dark` palette — the blue variant of the neutral dark
family — with the VS Code *key names* retained as the chrome-token
naming layer, remapped onto the new palette's slots (`editor.background
→ background #111113`, `sideBar.background → card #18191B`,
`button.background → primary #0090FF`, …). Dark Modern survives intact
as `theme::dark_modern()`; its 82 calibrated tokens and the
`register_workbench_tokens` retheme mechanism are paid for and proven
(slicer_match reskinned the whole crate in ~30 `with_token` lines).

Evidence, chose-because:
- Round 2: four home-turf agents told only "dense professional tool,
  VS Code workbench genre" all produced zinc + dense + flat + sparse
  accent — the trained prior's answer for this genre is not Dark
  Modern. Their measured chrome step (#18181B–#212124 over #09090B)
  matches radix_slate_blue_dark's card/background step (#18191B over
  #111113) to within a few 8-bit values.
- The surface model *inverts*: VS Code sinks chrome below the editor
  (#181818 under #1F1F1F); the shadcn genre raises chrome above content
  (card over background). The remap adopts the raised model.
- eutectic's binding ui-oracle is already zinc-family on workbench keys.
- The user's monitors render Dark Modern's low-contrast ramp poorly;
  the preference is the zinc family, specifically the blue variants.

REJECTED — Dark-Modern-as-default (the original §"Why VS Code
specifically" position): the corpus-priors argument held for the *key
vocabulary* but not the *values* — round-2 agents never reached for VS
Code names when shadcn slots were available, and value-level fidelity
to VS Code was never load-bearing for the four inversions. The key
names stay because they remain the only name source for chrome
(per the oracle registry); the values go because nothing but momentum
held them.

## `data_table` deferral lifted, 2026-07-28

The first-cut scope deferred `data_table` in core behind "hoist the
style-neutral skeleton when the second consumer actually lands." The
condition is met and the deferral is spent; `damascene_core::widgets::data_table`
ships. Recording it here so a future session reads the current law
rather than the deferral.

Chose-because:

- **Second consumer landed, and it is not eutectic.** The Raven kiosk —
  a dense instrument-panel desktop app — demands data-heavy tables on
  all 11 of its redesigned pages, and named `table`'s missing behaviour
  as its single largest widget gap. eutectic's netlists/DRC tables
  remain the third.
- **The measurement that motivated `TableColumn` kept going.** The
  geometry slice was taken early because four validation apps re-minted
  it; the same apps also re-minted the *behaviour* — `parts_v3.rs`
  scrolls its whole table, header and all, because there was no
  alternative, and its selection is a frozen `const SELECTED: usize`.
- **The skeleton turned out to need no core mechanism.** Sticky header
  and footer are a column of `[header, scroll(body), footer]` — the
  scroll is a sibling, not an ancestor. That is what made the hoist
  cheap enough to stop deferring: no `position: sticky` analogue, no
  layout-pass change, no new primitive.

What did *not* change: the style-neutrality rule. `data_table` is stock
shadcn vocabulary sized by the `ComponentSize` ladder, so the workbench
profile densifies it the same way it densifies everything else (`Xs`
lands a 29px row pitch, against the reference corpus's `tbody tr {
height: 28px }` plus its rule). The crate contributes values, not a
second table.

REJECTED again, and for the same reason as at ratification: putting the
table in `damascene-workbench` instead. The four toy-diagnosis signals
are values-and-recipes; a table's *behaviour* is neither, and shipping
it in the opinion crate would make row selection an aesthetic choice.
