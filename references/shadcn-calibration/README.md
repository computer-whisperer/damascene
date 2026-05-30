# Shadcn Calibration Reference

This side harness produces local reference screenshots for Damascene's polish
calibration loop. It is intentionally isolated from the Rust workspace.

Setup:

```bash
npm install
npm run build
```

Screenshots:

```bash
npm run capture
```

The capture script starts Vite on a free local port, opens Chromium through
Playwright, and pins the browser to deterministic scale contracts:

- stress viewport: `1180x780` CSS px by default,
- desktop viewport: `1440x900` CSS px by default,
- Playwright `deviceScaleFactor`: `1` by default,
- Chromium forced device scale factor: `1`,
- browser zoom: expected to remain at `1`,
- shadcn UI scale: controlled by root `font-size`, not desktop zoom,
- compact shadcn UI scale diagnostic: `0.875` by default,
- authored density references: compact, comfortable, and spacious at default
  shadcn UI scale.

Override these with environment variables:

```bash
SHADCN_REFERENCE_WIDTH=1440 \
SHADCN_REFERENCE_HEIGHT=900 \
SHADCN_REFERENCE_DESKTOP_WIDTH=1600 \
SHADCN_REFERENCE_DESKTOP_HEIGHT=1000 \
SHADCN_REFERENCE_PORT=5173 \
SHADCN_REFERENCE_DSF=1 \
SHADCN_REFERENCE_UI_SCALE=1 \
SHADCN_REFERENCE_COMPACT_UI_SCALE=0.875 \
npm run capture
```

Use `SHADCN_REFERENCE_UI_SCALE` to model app-level UI scale. Keep
`SHADCN_REFERENCE_DSF=1` unless the goal is explicitly testing raster
behavior; Damascene calibration compares logical layout first, then backend pixels.

The app also accepts a `density` query parameter:

```text
/?view=dashboard-01&density=compact
/?view=dashboard-01&density=comfortable
/?view=dashboard-01&density=spacious
```

This is an authored component/layout density axis. It changes control heights,
card padding, row heights, and gaps while keeping the root font scale fixed.
Do not treat it as a replacement for root UI scale diagnostics; the two answer
different questions.

Outputs:

- `out/shadcn-calibration.png` — local steelman for Damascene's first fixture.
- `out/shadcn-dashboard-01.png` — local dashboard-01-style density target.
- `out/shadcn-settings-01.png` — settings/form density and control target.
- `out/*.compact.png` — stress viewport at compact root UI scale diagnostic.
- `out/*.desktop.png` — canonical desktop viewport at default shadcn UI scale.
- `out/*.density-compact.png` — compact authored density at default UI scale.
- `out/*.density-spacious.png` — spacious authored density at default UI
  scale.
- matching `out/*.json` files — capture metadata with actual DPR, viewport,
  visual viewport scale, root font size, and `data-calibration-id`
  measurements.

The unqualified `out/shadcn-*.png` screenshots are the comfortable authored
density baseline.

Elements tagged with `data-calibration-id` are measured during capture. The
Damascene metric report pairs those DOM boxes with Damascene tree/draw-op metrics:

```bash
cargo run -p damascene-tools --bin make_calibration_metric_report
```

The reference app marks major surfaces with `data-calibration-boundary`.
`npm run capture` fails before writing a screenshot if a visible descendant
overflows one of those boundaries. This is intentionally similar to Damascene's
lint loop: broken reference screenshots should not become calibration targets.

The dashboard page is available at `/?view=dashboard-01`. It is modeled
after shadcn's dashboard block shape: sidebar, section cards, chart
region, recent activity, and dense data table.

The settings page is available at `/?view=settings-01`. It covers the
preference-pane shape: persistent sidebar, local section nav, profile fields,
selects, switches, checkbox rows, and secondary cards.
