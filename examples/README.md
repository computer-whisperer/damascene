# Damascene examples

Every bin here runs with

```sh
cargo run -p damascene-examples --bin <name>
```

against the winit + wgpu desktop host (a windowing environment is
required; the two stress bins are benchmarks and want `--release`).
Each file opens with a header comment saying what it proves and, where
it matters, which public surfaces it exercises — the bins are written
to be copy-paste starting points unless marked otherwise.

## Start here

| bin | what it shows |
| --- | --- |
| `counter` | The smallest interactive `App`-trait proof point. |
| `showcase` | Pages across six groups demoing every shadcn-shaped widget and every system capability (theme swap, animation, hotkeys, custom shaders, overlays, toasts). |
| `hero` | The polished end-to-end demo behind the README hero shot. |
| `settings_modal` | The canonical "modal + tabs + scroll body + sticky footer" form layout. |

## Widgets

| bin | what it shows |
| --- | --- |
| `tabs` | The controlled `tabs_list` widget driving a settings-style tabbed page. |
| `tooltip` | The `.tooltip(text)` modifier. |
| `popover` | The anchored popover widget. |
| `text_input` | The controlled single-line text widget. |
| `text_area` | The controlled multi-line text widget. |
| `text_selection` | Drag-select on static paragraphs and copy to the system clipboard. |
| `slider_keyboard` | The controlled `slider::apply_event` helper. |
| `hotkey_picker` | A hotkey-driven picker — the hotkey system end to end. |
| `scroll_list` | Selectable rows in a scroll viewport. |
| `virtual_list` | The virtualized list primitive. |
| `virtual_list_dyn` | Variable-height virtualization (`virtual_list_dyn`). |

## Plots, viewports, 3D

| bin | what it shows |
| --- | --- |
| `plot` | A 2D time-series plot inside an ordinary app — box-zoom, cursor, legend. |
| `viewport` | A pan/zoom `viewport()` over content larger than the window. |
| `scene3d` | A small polished 3D widget inside an ordinary app. |

## Accessibility

| bin | what it shows |
| --- | --- |
| `announce` | The canonical shape for screen-reader announcements. |
| `text_protocol` | The canonical shape for screen-reader-editable text. |

## Materials and paint

| bin | what it shows |
| --- | --- |
| `icon_gallery` | SVG-backed vector icons. |
| `icon_gallery_glass` | The vector-icon glass material. |
| `icon_gallery_relief` | The vector-icon relief material. |
| `liquid_glass_lab` | The liquid-glass material lab. |
| `custom_paint` | An app rendering its own geometry (a commit graph) through the paint stream — no parallel pipeline. |
| `animated_palette` | `.animate()` across a palette of tokens. |

## Integration and runtime

| bin | what it shows |
| --- | --- |
| `wgpu_integration` | Damascene inside an existing wgpu renderer sharing device, queue, and surface — the headline integration story. |
| `external_wakeup` | Push-driven redraw for event-class data. |
| `circular_layout` | The custom-layout escape hatch. |
| `settings` | The static `settings` fixture on the real backend (no interaction — fixture rendering only). |

## Internal benchmarks — not templates

| bin | what it shows |
| --- | --- |
| `structure_stress` | Structure-viewer frame-cost benchmark; exercises escape hatches most apps never need. |
| `conversation_stress` | Layout/paint cost on transcript-shaped trees. |
