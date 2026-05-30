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
