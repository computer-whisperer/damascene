# VS Code Calibration Reference

Color ground truth for the workbench look described in
`docs/WORKBENCH_VISION.md`. This directory holds VS Code's own default
dark theme definitions, fetched verbatim from the upstream repository —
no hand-transcribed values, no recalled hexes.

**Scope: color only.** Metrics (row heights, type sizes, paddings,
radii) are *not* in here and must not be inferred from these files.
Per the calibration plan in `docs/WORKBENCH_VISION.md`, metrics are
measured from screenshots of the real product at known scale; that step
is human-assisted and still pending. Indicative metric values anywhere
in the design docs remain hypotheses until those screenshots land.

## Source

Upstream: `microsoft/vscode`, pinned to release tag **`1.130.0`**
(published 2026-07-22), commit
**`1b6a188127eeaf9194f945eb6eb89a657e93c54c`**.

Fetched 2026-07-25. Exact URLs:

- `themes/dark_modern.json`
  <https://raw.githubusercontent.com/microsoft/vscode/1b6a188127eeaf9194f945eb6eb89a657e93c54c/extensions/theme-defaults/themes/dark_modern.json>
- `themes/dark_plus.json`
  <https://raw.githubusercontent.com/microsoft/vscode/1b6a188127eeaf9194f945eb6eb89a657e93c54c/extensions/theme-defaults/themes/dark_plus.json>
- `themes/dark_vs.json`
  <https://raw.githubusercontent.com/microsoft/vscode/1b6a188127eeaf9194f945eb6eb89a657e93c54c/extensions/theme-defaults/themes/dark_vs.json>

SHA-256 of the stored files:

```
7b0c9192ddb9c2208c24ceb118673645abbc87e8afabfb56c1ddff159ce73265  themes/dark_modern.json
409f33088a5cfd6589384900ef80627787c568d367c25b12f31bc7940936e573  themes/dark_plus.json
6ba2d945ed12a3c2ced075e4b04cde40931fbed9908c196b10fd732da7e31bc6  themes/dark_vs.json
```

Re-fetch (idempotent — should leave the tree clean):

```bash
SHA=1b6a188127eeaf9194f945eb6eb89a657e93c54c
for f in dark_modern.json dark_plus.json dark_vs.json; do
  curl -sS -o "references/vscode-calibration/themes/$f" \
    "https://raw.githubusercontent.com/microsoft/vscode/$SHA/extensions/theme-defaults/themes/$f"
done
```

## Include chain and resolution order

"Dark Modern" is VS Code's out-of-the-box dark theme; "Dark+" is the
older default, still shipped, and is Dark Modern's base. The chain:

```
dark_modern.json  ("Dark Modern")      include -> dark_plus.json
dark_plus.json    ("Dark+")            include -> dark_vs.json
dark_vs.json      ("Dark (Visual Studio)")   (chain root)
```

`include` means *the included theme is the base*. To resolve a value,
merge from the root of the chain outward, later files overriding
earlier ones — i.e. apply in this order:

1. `dark_vs.json`
2. `dark_plus.json`
3. `dark_modern.json`  ← wins

Below that sits a fourth layer that is **not** in this directory: keys
no theme in the chain sets fall back to the built-in color registry
defaults compiled into VS Code (see "Registry fallbacks").

What each layer actually contributes:

- `dark_vs.json` — 32 `colors` entries plus the bulk of the
  `tokenColors` (syntax) baseline.
- `dark_plus.json` — **no `colors` block at all**; it contributes only
  `tokenColors` and `semanticTokenColors`. For workbench chrome it is a
  pass-through link in the chain.
- `dark_modern.json` — 126 `colors` entries, no token colors. This is
  where essentially all workbench chrome color lives, and it overrides
  13 keys from `dark_vs.json` (`editor.background`, `menu.background`,
  `sideBarTitle.foreground`, `widget.border`, `checkbox.border`,
  `editor.foreground`, `input.placeholderForeground`,
  `menu.selectionBackground`, `activityBarBadge.background`,
  `sideBarSectionHeader.background`, `sideBarSectionHeader.border`,
  `statusBarItem.remoteBackground`, `statusBarItem.remoteForeground`).

Resolved chrome surface is therefore 145 distinct color keys.

### Parsing note

These files are **JSONC**, not strict JSON: they contain `//` comments
(`dark_plus.json`) and trailing commas (`dark_modern.json` ends with
`"widget.border": "#313131",\n\t},\n}`). Any tooling that reads them
must strip comments and tolerate trailing commas. Hex values appear in
mixed case and in 3-, 4-, 6-, and 8-digit forms (`#FFF`, `#0000`,
`#ccc3`, `#1F1F1F`, `#FFFFFF17`) — the short forms are RGB/RGBA
shorthand and the 8-digit forms carry alpha in the last byte.

## Registry fallbacks (cited, not vendored)

Several keys the workbench palette needs are set by *no* theme in the
chain; VS Code supplies them from
`src/vs/platform/theme/common/colors/listColors.ts` at the same commit:
<https://github.com/microsoft/vscode/blob/1b6a188127eeaf9194f945eb6eb89a657e93c54c/src/vs/platform/theme/common/colors/listColors.ts>

The `dark` variant of the relevant defaults:

| key | dark default | listColors.ts |
| --- | --- | --- |
| `list.activeSelectionBackground` | `#04395E` | L33–35 |
| `list.activeSelectionForeground` | `Color.white` | L37–39 |
| `list.inactiveSelectionBackground` | `#37373D` | L45–47 |
| `list.hoverBackground` | `#2A2D2E` | L65–67 |
| `list.focusBackground` | *(unset — no dark default)* | L17–19 |

These are transcribed from the cited source for convenience; the theme
JSONs in `themes/` are the only verbatim artifacts here. If a value
matters, re-read the cited file rather than trusting this table.
