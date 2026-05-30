<img src="https://raw.githubusercontent.com/computer-whisperer/damascene/main/assets/damascene_badge_icon.svg" alt="Damascene badge icon" width="96">

# damascene-vulkano

![Showcase — Settings section. The same fixture renders identically through wgpu and vulkano](https://raw.githubusercontent.com/computer-whisperer/damascene/main/assets/showcase_settings.png)

Native Vulkan backend for Damascene using `vulkano`.

Most applications should use `damascene-core` plus `damascene-winit-wgpu`.
Use this crate directly when validating backend parity or writing a
custom Vulkan host.

The public entry point mirrors `damascene-wgpu::Runner` where the GPU API
allows it. A host owns the window, device, queue, swapchain, and event
loop; the runner owns Damascene interaction state, layout/draw-op
preparation, Vulkan pipelines, text atlas images, and icon rendering.

WGSL remains the shader source language. This backend uses `naga` to
compile WGSL to SPIR-V when building pipelines.
