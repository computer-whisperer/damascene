# Damascene Shader Vision

This is the maintainer-facing architecture note for Damascene's rendering layer.
Public API guidance that should survive crates.io packaging belongs in crate
READMEs and rustdoc.

## Current Thesis

Damascene is a backend-neutral UI rendering system with shader-native materials.
`damascene-core` owns tree construction, layout, interaction state, draw ops,
theme resolution, and the backend-independent paint stream. Backend crates
turn that paint stream into concrete GPU commands.

The production path is GPU-first. SVGs, tree dumps, shader manifests, and
fixtures are feedback artifacts for maintainers and coding agents, not the
primary renderer.

## Load-Bearing Premises

1. **Core stays backend-neutral.** `damascene-core` must not depend on wgpu,
   vulkano, winit, or a specific application host.
2. **Backends own GPU integration.** `damascene-wgpu` and `damascene-vulkano` own
   device resources, pipelines, buffers, atlas upload, and render-pass details.
3. **Hosts can be simple or custom.** `damascene-winit-wgpu` packages the common
   native single-window path, but custom apps can integrate a backend runner
   directly into their own frame graph.
4. **Shaders are a first-class material surface.** Stock widgets should be easy
   to use, but downstream apps must be able to replace surface materials without
   forking the widget kit.
5. **Artifacts matter.** The agent loop needs readable tree, draw, shader, and
   lint artifacts so LLM authors can debug by inspecting packaged source and
   generated output.

## Crate Responsibilities

| Crate | Rendering responsibility |
|---|---|
| `damascene-core` | `El`, layout, widget kit, event helpers, text shaping inputs, draw ops, paint stream, themes, stock shader metadata, bundle artifacts. |
| `damascene-wgpu` | wgpu runner, pipeline cache, bind groups, buffers, glyph atlas upload, custom WGSL registration, backdrop snapshot implementation. |
| `damascene-vulkano` | vulkano runner with the same core paint contract, using naga/reflection where needed to keep shader source aligned. |
| `damascene-winit-wgpu` | optional native host that wires winit events and a wgpu surface into the wgpu runner. |
| Private fixtures/tools | visual calibration, backend demos, reference shaders, and repo-only diagnostics. |

The publishable API should make sense when an LLM reads only the downloaded
crate source from cargo. Private fixture crates must not be required to learn
the public surface.

## Paint Pipeline

The current pipeline is:

```text
App::build / El tree
    -> layout and UiState update
    -> DrawOp stream
    -> RunnerCore::prepare_paint
    -> PaintItem stream
    -> backend prepare/render/draw
```

`RunnerCore::prepare_paint` is the cross-backend boundary. It resolves theme
materials, packs stock/custom shader slots, batches compatible quads, carries
text/icon items, and inserts a `BackdropSnapshot` before the first paint item
whose shader samples the backdrop.

This split is important: adding a backend should mostly mean implementing the
paint-stream consumer, not reimplementing widget semantics.

## Shader Model

Stock rendering is uniform-driven:

- `SolidQuad` is the simplest flat-color fallback.
- `RoundedRect` handles most surfaces: fills, borders, radius, shadows, focus
  treatment, and role-specific material slots.
- `DividerLine` handles crisp separators.
- `Text` renders glyph atlas masks produced by the core text path.
- Icon/vector shaders support the stock icon vocabulary and richer icon
  materials.

The rule is uniform proliferation before shader proliferation. A new card
variant should usually be a new uniform recipe or theme role override, not a
new stock shader.

Custom shaders use `ShaderBinding::custom(...)` and generic vector slots.
Themes can route implicit surface roles to custom shaders with
`Theme::with_surface_shader`, `Theme::with_role_shader`, and matching uniform
overrides. That gives app authors a stable widget vocabulary while letting
theme packages replace the material implementation.

## Shader Language

WGSL is the current source format. The vulkano path uses naga/reflection so the
same shader source can feed both backend families where possible.

Do not add a second shader language until a real app hits a composition wall
that cannot be solved with stock shader factoring, uniforms, or helper WGSL.
Slang or another composition-oriented language remains an escalation option,
not a near-term dependency.

## Backdrop Sampling

Backdrop sampling is opt-in shader metadata:

```rust
runner.register_shader_with("glass", source, true)?;
```

Shaders that sample the backdrop bind:

```wgsl
@group(1) @binding(0) var backdrop_tex: texture_2d<f32>;
@group(1) @binding(1) var backdrop_smp: sampler;
```

The backend contract is:

- A backend must create the extra bind group/layout for shaders registered with
  `samples_backdrop = true`.
- The host color target must allow copying from the current frame image.
  wgpu targets need `COPY_SRC`; vulkano swapchain images need
  `TRANSFER_SRC`.
- Current core scheduling emits at most one snapshot per frame, immediately
  before the first backdrop-sampling item.

Multiple nested backdrop layers are a future extension. The current contract is
deliberately smaller because one snapshot is enough to validate frosted/glass
materials without overdesigning the scheduler.

## Host Integration

Simple native apps should use `damascene-winit-wgpu`:

```rust
use damascene_core::prelude::*;
use damascene_winit_wgpu::run;

fn main() -> anyhow::Result<()> {
    run(MyApp::default())
}
```

Custom hosts should integrate a backend runner directly. The host still owns
the window, surface, swapchain, present timing, larger render graph, and any
non-Damascene rendering. Damascene can provide layout-computed keyed rects so the host
can paint 3D views, video panes, graphs, or other custom regions in the same
frame.

This is why `damascene-core` is not an app framework. It is the rendering,
layout, and interaction system. Host crates package common app structure.

## Artifact Contract

For coding agents working on Damascene, the important artifacts are:

- tree dumps showing layout, keys, roles, state, and text overflow policy,
- draw/paint dumps showing shader choices and packed material slots,
- shader manifests and compile diagnostics,
- PNG/SVG snapshots for visual comparison,
- lint findings for raw colors, overflow, weak focus, and other polish issues.

These artifacts should explain the rendered output without requiring an agent
to reverse-engineer a backend runner.

## Open Design Pressure

- Keep `damascene-core` free of backend and windowing dependencies.
- Keep public shader APIs documented in crates that survive packaging.
- Avoid a broad `Painter` abstraction unless a second/third backend proves the
  current paint-stream boundary is not enough.
- Treat private fixtures as tests and calibration aids, not public examples.
- Use real app ports, especially whisper-git, to decide which shader and theme
  APIs are stable enough for an initial release.
