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

## GPU-setup failures

Adapter/device acquisition is async and finishes after the wasm
`init()` promise resolves, so a browser with neither usable WebGPU nor
WebGL2 cannot reject `init()`. The host reports such failures as a
bubbling `damascene-error` `CustomEvent` on the canvas with
`detail = { kind: "gpu-setup", message }`:

```js
canvas.addEventListener('damascene-error', (e) => {
    showFatalError(e.detail.message);
});
```

## SPA embedding

A full-page canvas can ignore the handle's lifetime. When an SPA
framework mounts and unmounts the canvas, pair every mount with
`WebHandle::destroy()` on unmount — it stops the event loop,
unregisters the host's DOM listeners and `ResizeObserver`, removes
the hidden soft-keyboard input from `<body>`, and releases the GPU
surface. Without it each remount leaks the previous host. After
`destroy()`, calling `start_with` again (a fresh canvas with the same
id is fine) creates an independent host.

The repository's browser showcase lives in the unpublished
`damascene-web-showcase` crate.

## Profiling

Enable the `profiling` feature to route every `profile_span!` call
through `tracing-wasm`. Spans land on the browser's User Timing API
(`performance.measure`); record a profile in DevTools → Performance
and the Damascene spans appear as labeled measures in the flamegraph
alongside the page's frame/script work.
