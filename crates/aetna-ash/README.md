<img src="https://raw.githubusercontent.com/computer-whisperer/aetna/main/assets/aetna_badge_icon.svg" alt="Aetna badge icon" width="96">

# aetna-ash

Low-level `ash`/Vulkan renderer adapter for Aetna.

This crate is for hosts that already own an `ash` renderer: compositor
HUDs, custom Wayland clients, engines, and other frame graphs where the
application owns the Vulkan instance, device, queues, images, command
buffers, synchronization, surface, swapchain, and event loop.

`aetna-ash` is not a windowing or Wayland host. It owns only the Aetna
runtime/rendering side: interaction state, layout/draw-op preparation,
stock/custom shader registration, and eventually the Vulkan resources
needed to draw those paint streams through `ash`.

The intended host shape mirrors `aetna-vulkano` at a lower level:

1. Create your ash instance/device/queues/swapchain/frame graph.
2. Create an `aetna_ash::Runner` from an `AshContext` and target info.
3. Forward input as `aetna_core` pointer/key/text events.
4. Build an `El` tree and call `prepare`.
5. Record Aetna into your command buffer with `draw` or `render`.

`draw` is the integration point for hosts that already opened a
compatible Vulkan dynamic-rendering scope and do not need backdrop
sampling. `render` is reserved for Aetna-owned dynamic rendering/pass
splitting, including backdrop sampling custom shaders.
