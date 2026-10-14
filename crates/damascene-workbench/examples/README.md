# Workbench examples

Two demos and twelve measurement artifacts. The method they came from is
`docs/NAMING_ORACLE.md` §"The metric": a fresh agent is given the library
and a brief, nothing else, and what it writes is the measurement.

## Demos

- `shell.rs` — a VS Code-ish shell on the workbench theme; carries the
  crate's whole-shell lint guard (`test = true`).
- `data_table.rs` — the behaving table in three views; also `test = true`.

## The four briefs

Each round gave the same four briefs to fresh agents. Reconstructed from
the base examples' headers and the round-1 references' titles (the
original brief text lived in the fork author's session and did not come
over with the merge); use these, verbatim, for the next round.

1. **Preferences — Appearance** (`settings*.rs`). A full-window
   preferences page: a slim header strip with a search field, a category
   rail, and a capped-width content column of setting groups. Every
   control is a stock widget rendered through the workbench theme.
2. **Meridian Slicer** (`slicer*.rs`). A static mock of a 3D-printer
   slicer: title/menu strip, a narrow tool rail, a dark 3D canvas, a
   dense right-hand settings panel, a status bar.
3. **Copperline — Parts Library** (`parts*.rs`). An ECAD parts-library
   browser: a filter toolbar across the top, a dense table of parts
   filling the work area, a properties inspector docked right, a status
   bar. Selects open and pick, fields type, clicking a row moves the
   selection; nothing is persisted.
4. **Banter — Build Room · Northwind Collective** (`voice*.rs`). A
   voice-chat client in the Mumble / TeamSpeak genre: a channel tree
   with presence, a message log, a composer, a status bar. Rosters and
   levels, not documents.

## Rounds

| round | files | what it measured |
|---|---|---|
| 1, blind (2026-07, `e99981f`) | `settings.rs`, `slicer.rs`, `parts.rs`, `voice.rs` | first-guess rate and helper count with the vocabulary as it was: 13–21 local helpers per app |
| target-match (`527e160`) | `*_match.rs` | screenshot-steered reproductions of the **round-1** references (`index.html`), as far as the stock vocabulary reached; found the icon ceiling and the 1px coverage bug |
| 2, blind (`d507a87`) | `*_v2.rs` | the same briefs after the first anatomy widgets |
| 3, blind (`4189e22`) | `*_v3.rs` | the anatomy arc's acceptance run: parts 21 → 12 helpers, slicer 449 → 414 lines and 5 → 0 raw colours |

## Reference provenance

`references/workbench-validation/<app>/index.html` is the round-1
blank-slate HTML reference each `*_match.rs` reproduces (slate-blue ramp
`#1E2023` / `#1A1C1F` / `#24272B`). `reference.png` in the same folder
is **not** its render: `064d1e7` replaced all four PNGs with renders of
the round-2 references, the real shadcn + Tailwind builds under
`shadcn-refs/src/views/` (zinc `#09090B`). The `*_match.rs` headers
that name `reference.png` as their target predate that swap.
