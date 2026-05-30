# Damascene Library Vision

This is the maintainer-facing architecture note for Damascene's application and
widget layer. Public author guidance belongs in crate READMEs and rustdoc so it
is visible after crates.io packaging.

## Current Thesis

Damascene is a small declarative UI library for native GPU applications:

```text
host state -> App::build -> El tree -> layout/interactions -> GPU paint
```

The host owns application state and lifecycle. Damascene owns layout, hit testing,
event routing, widget composition, visual state, text/icon rendering, and the
backend-neutral paint stream.

This is intentionally narrower than a full app framework. The reusable core is
the rendering and interaction system; helper host crates can package common
window/event-loop setup.

## Public Surface For LLM Authors

The API should be learnable by an LLM from cargo-downloaded source. The most
important visible surfaces are:

- `damascene_core::prelude::*`
- the `App` trait,
- `El` builders and modifiers,
- stock widgets and controlled-widget helpers,
- `UiEvent` action helpers such as `is_click_or_activate`,
- `Key`/identity conventions,
- theme and shader registration APIs,
- backend runner and host-crate entry points.

A small counter should look like this:

```rust
use damascene_core::prelude::*;

#[derive(Default)]
struct Counter {
    value: i32,
}

impl App for Counter {
    fn build(&self, _cx: &BuildCx) -> El {
        column([
            h1(format!("{}", self.value)),
            row([
                button("-").key("dec"),
                button("Reset").key("reset"),
                button("+").key("inc"),
            ]),
        ])
    }

    fn on_event(&mut self, event: UiEvent) {
        if event.is_click_or_activate("inc") {
            self.value += 1;
        } else if event.is_click_or_activate("dec") {
            self.value -= 1;
        } else if event.is_click_or_activate("reset") {
            self.value = 0;
        }
    }
}
```

The author should not need to learn a state framework, component lifecycle, or
backend render-pass mechanics for a simple app.

## What Core Owns

`damascene-core` owns:

- tree construction through `El`,
- layout primitives: `column`, `row`, `stack`, `Hug`, `Fill`, fixed sizing,
  gap, padding, alignment, clipping, and scroll regions,
- hit testing and focus/hover/press tracking,
- event routing through `UiEvent` and app callbacks,
- stock widget composition,
- controlled input state passed through app-owned fields,
- text shaping inputs, overflow policy, and text roles,
- icon vocabulary and icon draw ops,
- shader bindings, theme resolution, and draw-op preparation,
- backend-neutral bundle artifacts.

Core should remain usable without winit, wgpu, vulkano, or a specific app host.

## What Core Does Not Own

Core does not own:

- application state storage,
- persistence, undo/redo, or data loading,
- networking, filesystem watching, or background workers,
- window creation or main-loop policy,
- swapchain/present timing,
- multi-window, tray, menu-bar, or platform integration,
- non-UI rendering.

The host mutates its own state, requests redraws, and calls into Damascene to
project that state into UI.

## Controlled Widgets

Stock widgets should be controlled by app state rather than hidden widget
instances. For example, text inputs, selection, checkboxes, toggles, sliders,
and segmented controls should expose clear value/event surfaces. `UiState`
exists for transient UI mechanics such as hover, press, focus, scroll, and
advanced per-widget state. It should not become a parallel app-state store.

The widget kit has one important invariant: stock widgets must be built from
the same public `El` surface that app authors use. If a stock widget needs
private powers that normal authors cannot reach, either the public surface is
missing something or the widget is doing too much.

## Escape Hatches

Damascene has two intended escape hatches:

- **Custom shader:** change the visual material when stock surfaces are not
  enough.
- **Custom layout:** place children using app/domain-specific geometry when
  row/column/stack are not enough.

Host-composed rendering is a consequence of the host split, not a third widget
primitive. A host can reserve a keyed region in the UI tree, ask for its
computed rect, and paint its own 3D viewport, graph, video frame, or GPU view
with whatever pipeline it owns.

## Crate Layering

The intended publishable set is:

- `damascene-fonts`
  - split font asset crates such as `damascene-fonts-inter`,
    `damascene-fonts-jetbrains-mono`, `damascene-fonts-emoji`,
    `damascene-fonts-symbols`, and `damascene-fonts-roboto`
- `damascene-core`
- `damascene-html` (markdown's optional HTML pass-through; also a registry
  prerequisite, since crates.io requires every listed dependency — optional
  included — to already exist)
- `damascene-markdown`
- `damascene-wgpu`
- `damascene-winit-wgpu`
- `damascene-vulkano`
- `damascene-ash` (Vulkan-via-`ash` backend, peer to `damascene-vulkano`)
- `damascene-web` (wasm browser host over `damascene-wgpu`)

Private crates such as fixtures, tools, demos, web experiments, and reference
apps can pressure-test the architecture, but they should not be part of the
author-facing dependency story unless they become genuinely reusable.

## Documentation Rule

Docs in this directory coordinate agents and maintainers working on Damascene.
Crate-level READMEs and rustdoc teach downstream authors.

When an API is intended for public use, document it where a cargo user will see
it. That means:

- crate README for the crate-level role and first example,
- rustdoc on the trait/type/method,
- examples in publishable crates when the behavior is crate-local,
- root `examples/` only when demonstrating combined behavior across crates.

## Near-Term Stability Questions

Before porting serious apps, review these surfaces as if an LLM author is
using only packaged crate source:

- Is `App` minimal and obvious enough?
- Are event helpers named around author intent rather than raw event shape?
- Are controlled widget APIs consistent across text, selection, toggle, slider,
  menu, and command-palette style interactions?
- Are keys and keyed-rect lookup documented enough for host-composed regions?
- Are theme role overrides understandable without reading private fixtures?
- Are custom shader registration and uniform slots documented in the backend
  crate where the author uses them?
- Do examples live in the crate that provides the API they demonstrate?

## What This Is Not

- Not reactive: the host owns state changes and redraw requests.
- Not retained widget instances: stable identity comes from keys and internal
  UI trackers.
- Not a game engine: no ECS or general scene graph.
- Not a windowing framework: host crates package common setup, but core and
  backend crates stay embeddable.
- Not an agent framework: artifacts help agents, but Damascene does not prescribe
  an agent workflow.
