<img src="https://raw.githubusercontent.com/computer-whisperer/damascene/main/assets/damascene_badge_icon.svg" alt="Damascene badge icon" width="96">

# damascene-vulkano

![Showcase — Text & value section. The same fixture renders identically through wgpu and vulkano](https://raw.githubusercontent.com/computer-whisperer/damascene/main/assets/showcase_text_inputs.png)

Native Vulkan backend for Damascene using `vulkano`.

Most applications should use `damascene-core` plus `damascene-winit-wgpu`.
Use this crate directly when validating backend parity or writing a
custom Vulkan host.

The public entry point mirrors `damascene-wgpu::Runner` where the GPU API
allows it. A host owns the window, device, queue, swapchain, and event
loop; the runner owns Damascene interaction state, layout/draw-op
preparation, Vulkan pipelines, text atlas images, and icon rendering.

## Swapchain requirements

Create the swapchain with

```text
image_usage: COLOR_ATTACHMENT | TRANSFER_SRC | TRANSFER_DST
```

`TRANSFER_SRC` lets `Runner::render` copy the mid-frame surface into its
backdrop snapshot for backdrop-sampling shaders (modal scrim blur, …).
`TRANSFER_DST` is required under MSAA: vulkano's render-pass macro gives
the resolve attachment a `TransferDstOptimal` layout, which needs the
flag on the underlying image. The runner validates this on the first
frame and fails with an actionable message; most surfaces advertise both
flags, so the cost of always setting them is nil. See
`damascene-vulkano-demo` for a complete bring-up.

WGSL remains the shader source language. This backend uses `naga` to
compile WGSL to SPIR-V when building pipelines.
