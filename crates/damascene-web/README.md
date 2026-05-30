# damascene-web

Reusable browser host for Damascene wasm apps.

Write UI code against `damascene-core::prelude::*`, then call
`damascene_web::start_with` from your wasm crate's own
`#[wasm_bindgen(start)]` entry point.

The host provides:

- a `<canvas id="damascene_canvas">` supplied by the host page
- `winit`'s web event loop
- `damascene-wgpu` rendering through browser WebGPU/WebGL support
- clipboard, keyboard, pointer, resize, cursor, toast, focus, scroll,
  link-open, shader, and theme plumbing for any `damascene_core::App`

Use `start_with_config` to target a different canvas id. Keep the
returned `WebHandle` in external browser callbacks when they need to
push work into app-owned state and request a redraw.

The repository's browser showcase lives in the unpublished
`damascene-web-showcase` crate.

## Profiling

Enable the `profiling` feature to route every `profile_span!` call
through `tracing-wasm`. Spans land on the browser's User Timing API
(`performance.measure`); record a profile in DevTools → Performance
and the Damascene spans appear as labeled measures in the flamegraph
alongside the page's frame/script work.
