<img src="https://raw.githubusercontent.com/computer-whisperer/damascene/main/assets/damascene_badge_icon.svg" alt="Damascene badge icon" width="96">

# damascene-wgpu

![Liquid-glass section — custom shader sampling the wgpu backdrop](https://raw.githubusercontent.com/computer-whisperer/damascene/main/assets/showcase_glass.png)

`wgpu` backend for Damascene.

Most applications should not start here. Implement `damascene_core::App` and
run it through `damascene-winit-wgpu` for a native window.

Use this crate directly when you are writing a custom host or embedding
Damascene into an existing `wgpu` render loop:

1. Create a `Runner` with the target texture format.
2. Register any app shaders.
3. Forward pointer, keyboard, text-input, modifier, and wheel events to
   the runner.
4. Call `prepare` with a fresh `El` tree before drawing.
5. Call `render` when Damascene owns pass boundaries, especially for
   backdrop-sampling shaders; call `draw` only inside a pass you own and
   only when backdrop sampling is not needed.

Coordinates passed to interaction methods are logical pixels. Render
targets are physical pixels; pass the host scale factor to `prepare`.

## Custom-host checklist

Contracts every out-of-tree host (raw Wayland layer-shell bars,
notification daemons, multi-window composit-or-bust setups) has had to
reverse-engineer. Work through these once and your host matches the
in-tree winit host's behavior:

**Surface format.** Prefer an sRGB swapchain format (`*UnormSrgb`) so
the hardware encodes on write; create the `Runner` with the same format
you configure the surface with.

**Alpha mode.** Damascene's blending assumes the compositor reads
**premultiplied** alpha when the surface is transparent. Negotiate
`CompositeAlphaMode::PreMultiplied` when the surface capabilities offer
it; if you fall back to `Opaque`, transparent themes will composite
incorrectly — log it rather than failing silently.

**Surface loss.** `get_current_texture()` returning `Lost` / `Outdated`
means reconfigure-and-retry: reconfigure the surface with the current
size, mark the frame dirty, and try again next loop iteration. Expect a
burst of these during interactive resize on Wayland; coalesce resizes so
`surface.configure()` runs once per frame, not once per event.

**Scale factor.** Layout runs in logical pixels: pass a logical-size
viewport to `prepare` along with the scale factor; configure the wgpu
surface at physical size; convert pointer coordinates to logical before
forwarding. On raw Wayland, also call `wl_surface::set_buffer_scale`
when the output scale changes, then reconfigure.

**Glyph warming.** `Runner::warm_default_glyphs()` pre-rasterizes the
ASCII set so the first text frame doesn't hitch — ~40 ms optimized, but
**~19 s in an unoptimized debug build** (measured; see the dev-profile
section in the workspace README for the fix). Never call it inside a
Wayland dispatch callback in a debug build: starving the socket that
long gets you disconnected by the compositor. Multi-window hosts: warm
a `SharedText` pool once per device instead (next item).

**Shared text atlases.** A `Runner` built with `Runner::new`/`with_caps`
owns private glyph/MSDF atlases — N windows pay N× atlas VRAM, N×
glyph rasterization, and N× warmup. Create one `SharedText::new(&device)`
per device, `warm_default_glyphs()` it once, and build every window's
runner with `Runner::with_shared_text(.., &pool)`: fonts, the shaping
cache, and the atlas GPU pages are then shared (the pool is
format/sample-count independent, so mixed SDR/HDR windows share too).
An existing runner's pool is reachable via `runner.shared_text()`.
Composes with the pooled-`Runner` window-open path
(`WindowGfx::with_surface_and_renderer`): build pooled runners from the
shared pool and both costs collapse.

**Runner pooling.** A `Runner` is not bound to any surface or window —
it depends only on the (target format, sample count) it was built
with, and `render` takes the target texture every frame. A resident
multi-window daemon can build and warm Runners off the open path
(`Runner::with_caps` + `warm_default_glyphs`, ~360 ms measured) and
re-point one at each new window (`set_target_format` /
`set_surface_size` / `set_working_color_space` /
`set_output_luminance`), making window-open latency negotiation-only.
winit hosts get this packaged as
`damascene_winit_wgpu::host::WindowGfx::with_surface_and_renderer`.

**Per-frame calls.** `set_theme`, `set_hotkeys`, `set_selection`, and
the `push_*` request drains are cheap per-frame snapshots — call them
every frame before `prepare`, in any order.

**Hotkeys per window.** The hotkey registry is per-`Runner`, and a
multi-window host owns one `Runner` per window — feed each window's
`set_hotkeys` only that window's list and route key events by window.
A chord then fires in the OS-focused window only; for app-global
accelerators, register the chord in every window's list and treat the
per-window `Hotkey` event as one action.

**Redraw scheduling.** `prepare()` returns `next_layout_redraw_in`
(animations settling, tooltip fades — needs a full rebuild + layout)
and `next_paint_redraw_in` (time-driven shaders — `Runner::repaint`
reuses cached ops). Schedule timers from both; `None` means no future
frame is needed until input arrives.

**Measure passes.** To size a surface before it exists (layer-shell
daemons sizing to content), run `damascene_core::layout::layout` against
a **separate `UiState`** — reusing the live runner's state would leak
hover/focus/animation state between the headless measure and the real
frame.

**Drop order.** A wgpu surface created from a raw window handle borrows
the native surface it was created over. Declare the wgpu surface field
*before* the native surface/window in your struct (Rust drops fields in
declaration order) or document the manual teardown order.

**Input mapping.** `damascene_core::PointerButton::from_linux_button`
maps evdev `BTN_*` codes for raw Wayland hosts;
`damascene_core::Cursor::css_name()` bridges cursors to any windowing
layer (winit's `CursorIcon` parses the CSS names directly). winit hosts
can reuse the pure mappers in `damascene_winit_wgpu::host::input`.
