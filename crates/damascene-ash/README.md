<img src="https://raw.githubusercontent.com/computer-whisperer/damascene/main/assets/damascene_badge_icon.svg" alt="Damascene badge icon" width="96">

# damascene-ash

Low-level `ash`/Vulkan renderer adapter for Damascene.

This crate is for hosts that already own an `ash` renderer: compositor
HUDs, custom Wayland clients, engines, and other frame graphs where the
application owns the Vulkan instance, device, queues, images, command
buffers, synchronization, surface, swapchain, and event loop.

`damascene-ash` is not a windowing or Wayland host. It owns only the Damascene
runtime/rendering side: interaction state, layout/draw-op preparation,
stock/custom shader registration, and the Vulkan resources needed to
draw supported paint streams through `ash`.

The intended host shape mirrors `damascene-vulkano` at a lower level:

1. Create your ash instance/device/queues/swapchain/frame graph.
2. Create an `damascene_ash::Runner` from an `AshContext` and target info.
3. Forward input as `damascene_core` pointer/key/text events.
4. Build an `El` tree and call `prepare`.
5. Record Damascene into your command buffer with `draw` or `render`.

`draw` is the integration point for hosts that already opened a
compatible Vulkan dynamic-rendering scope and do not need backdrop
sampling. `render` opens Damascene-owned dynamic rendering around the same
paint stream. App-owned textures can be wrapped with
`damascene_ash::app_texture` when the host keeps the image/view alive and in
`SHADER_READ_ONLY_OPTIMAL` for Damascene's draw, or
`damascene_ash::app_texture_with_layout` when the host uses another
shader-readable layout. Backdrop sampling is not implemented yet.
