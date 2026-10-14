# workbench-validation: shadcn reference apps

Round-2 web references for the workbench validation corpus — the same
four app briefs as the round-1 blank-slate mocks (`../*/index.html`),
rebuilt on **real shadcn/ui + Tailwind** so both sides of the
comparison play their trained compressed grammar (see
`docs/NAMING_ORACLE.md`, "Reference targets").

## Provenance (pinned)

- Scaffold: `npm create vite@latest -- --template react-ts`, React 19,
  Vite 7, Tailwind v4 via `@tailwindcss/vite`.
- Components: `shadcn` CLI **4.15.0**, radix library
  (`import { … } from "radix-ui"` generation), fetched 2026-07-26 into
  `src/components/ui/` — vendored verbatim, do not hand-edit.
- Theme: zinc `cssVarsV4`, fetched verbatim from
  `https://ui.shadcn.com/r/colors/zinc.json` into `src/index.css`.
  Dark class is applied globally in `main.tsx`.

## Usage

```bash
npm install
npm run build
npm run preview -- --host 127.0.0.1 --port 4183   # IPv4 pin matters
# screenshot at exactly 1280x800:
google-chrome-stable --headless=new --disable-gpu --hide-scrollbars \
  --force-device-scale-factor=1 --window-size=1280,800 \
  --screenshot=out.png "http://127.0.0.1:4183/?view=slicer"
```

Views: `?view=slicer|voice|parts|settings`, each designed for exactly
1280x800 (the `App.tsx` frame clips to that box).
