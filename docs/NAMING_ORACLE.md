# The Naming Oracle Registry

Ratified 2026-07-26. Companion to `docs/WORKBENCH_VISION.md` and
`docs/VOCABULARY_PARITY.md`; this document extends the calibration
discipline from **values** to **names**.

## Premise

Damascene's target is not "a UI framework in Rust" — it is a grammar
that lets an LLM author emit **high-entropy output**: every token the
agent writes should be decision-bearing, with everything predictable
absorbed by defaults. A blank slate (plain HTML+JS) parses everything
but makes the correct result cost maximal output; a bespoke framework
is compact but the agent's first guess misses. Damascene's bet is to
take both halves by stealing the grammar the agent is already trained
on: **the agent's natural first guess should parse, and the shortest
parse should be the polished result.**

That only works if names come from a consistent, corpus-dominant
source per layer. The 2026-07 validation rounds measured the failure
mode directly: every "path I only found by reading source" report was
either a missing production or a name that diverged from the layer's
oracle.

## The registry

| layer | oracle | notes |
|---|---|---|
| widget & anatomy names | **shadcn/ui**, tracked at a pinned registry version | component names, slot names, variant names. shadcn's late-2025 generation (`Field`, `InputGroup`, `ButtonGroup`, `Kbd`, `Spinner`, `Item`, `Empty`) is the ratified spec for the anatomy arc — most of our measured anatomy gaps are components shadcn has since named. |
| modifier / utility vocabulary | **Tailwind CSS** | `.gap`, `.radius`, `.max_width`, text roles. Divergences are auditable findings (e.g. `TextAlign::Start/End` vs `text-left/right` — mitigated with doc aliases, 2026-07). |
| icon names | **Lucide** | ratified de facto 2026-07-26: all four validation agents reached for Lucide names verbatim; the 26→57 expansion adopted them wholesale, path data verbatim ISC. |
| workbench chrome recipes | **VS Code part names** | `activitybar`, `sidebar`, `panel`, `statusbar`, `auxiliarybar`, `titlebar` — the only name source that exists for this layer. Theme token *values* continue to cite VS Code theme keys verbatim (see `references/vscode-calibration/`). |
| web-platform vocabulary | **HTML/CSS itself** | the standing exception for things frameworks under-expose but the platform names (`<meter>`, per-side borders, `content_inset`). |

Morphology translates (kebab/JSX → snake/builder Rust); **semantics do
not**. `IconName::Mic` is Lucide's `mic` in Rust case — the agent
translates case conventions effortlessly, and misses when the semantic
name itself differs.

## Policy

- Every new public name cites its oracle (a doc line, same as tokens
  citing their VS Code key). "No oracle" is allowed only with a
  recorded justification in the PR/commit.
- Oracles are **pinned**: shadcn at a recorded registry snapshot,
  Lucide at a recorded tag (0.460.0 for `flip-horizontal`, which
  upstream later dropped). Tracking upstream additions is deliberate,
  scheduled work, not drift.
- A divergence discovered later is a finding, not a taste debate:
  either rename (pre-1.0), alias, or record why the oracle loses.
- Where shadcn's own component is a **recipe over a headless library**
  (its `DataTable` is TanStack Table wired to the `Table` primitives),
  the recipe's vocabulary is the oracle for the part we ship, and the
  citation names both. Hoisting the geometry half of a recipe without
  its state half is a legitimate move — it is recorded as such rather
  than renamed to avoid the collision.

### Recorded citations

| our name | oracle | note |
|---|---|---|
| `resizable_panel_group` / `resizable_panel` / `resizable_handle` (+ `_with_grip`) | shadcn **`Resizable`** (`ResizablePanelGroup` / `ResizablePanel` / `ResizableHandle`), a recipe over **react-resizable-panels** (`PanelGroup` / `Panel` / `PanelResizeHandle`) | Per the recipe-over-headless-library row, the citation names both. We hoist the geometry-and-interaction half; the library's internal percentage state becomes the house controlled pattern (app-owned `Vec<f32>` fractions + `resizable::apply_event`). Recorded divergences: `direction` → `Axis::Row`/`Column` (the `resize_handle` precedent); `withHandle` → `resizable_handle_with_grip` (literal morphology is the degenerate `handle_with_handle`); per-panel `minSize` simplified to one group-wide `ResizableOpts::min_fraction` (default 0.1 = the library's default `minSize` 10%). Deliberately the **linear** shape only — the recursive split tree stays app-side per `docs/WORKBENCH_VISION.md`'s deferral. |
| `TableColumn` (`width`, `align`, `fill`/`fixed`/`align_end`) | TanStack Table / shadcn `DataTable`'s **`columns`** array | The corpus-dominant name for "one positional spec the header and every body row render from" — the thing four validation agents re-minted as a local `cell(content, weight)` / `head(label, weight)` pair. Deliberately **geometry-only**: no `sortingFn`, no `enableSorting`, no row-selection model, no accessors — those live on `DataColumn` (below), which holds a `TableColumn` rather than re-minting its geometry. `TableColumn` is the width/alignment slice of the skeleton, taken early because its absence was measured twice. `table_header_cells` / `table_row_cells` are ours — shadcn has no name for them, since on the web the `columns` array is consumed by `flexRender` rather than by a pair of constructors. |
| `data_table` / `DataColumn` (`header`, `sortable`) / `DataTableState` / `data_row` / `data_detail_row` / `data_group_row` / `data_footer_row` | shadcn's **`DataTable`** recipe over **TanStack Table** (`columns`, `getRowId`, `enableSorting` + `SortingState`, `getExpandedRowModel` / `renderSubComponent`, grouping, sticky header/footer) | The recipe-over-headless-library row again: the citation names both, we hoist the anatomy-and-interaction half, and TanStack's internal state (`sorting`, `rowSelection`, `expanded`) becomes the house controlled pattern — app-owned `DataTableState` + `data_table::apply_event`. Deliberate divergences: **no sorting** (`Sort` records what was asked; the app owns the comparator, since the widget sees `El`s and not rows); *one* flat visible-row list rather than TanStack's row-model pipeline, matching `tree`'s flattening rule and keeping expansion in the app; sticky header/footer by composition (siblings of an internal scroll) rather than `position: sticky`, which core has no analogue for. `data_row_key` / `data_sort_key` / `data_expand_key` / `data_body_key` are ours — the routed-key convention (`key::indexed`) has no web counterpart. Out of v1 with the shape genuinely undecided: column resize dragging, cell editing, horizontal scrolling, multi-select ranges. |

## The metric

**First-guess rate.** The validation instrument (blind and
target-match agent rounds, `references/workbench-validation/`)
produces a miss log for free — every source-dive entry is a failed
first guess against our grammar. Alongside it, the **helper-function
count** in blind-round examples measures missing productions: a local
helper is the agent minting private grammar because the public one
lacked a production (measured 2026-07: 13–21 per app; the anatomy
arc's acceptance test is collapsing that toward natural decomposition
on a fresh blind run).

## Reference targets

The web reference implementations in `references/workbench-validation/`
migrate from blank-slate HTML/CSS (round 1, kept as the fluency
baseline) to **real shadcn + Tailwind** builds — both sides then play
their trained compressed grammar, making token-for-token entropy
comparison fair and the reference the true target vocabulary. Pixel
tolerance is deliberately looser across this migration: it creates a
new target rather than refining the old one.
