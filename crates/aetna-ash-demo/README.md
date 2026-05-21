# aetna-ash-demo

Native `ash` smoke harness for `aetna-ash`.

This is intentionally smaller than `aetna-winit-wgpu`: it is a demo and
test harness for hosts that already expect to manage Vulkan directly.
The crate owns a winit window, ash Vulkan 1.3 instance/device/queue,
surface, swapchain, command buffer, simple one-frame synchronization,
and winit input translation. The reusable integration surface remains
`aetna-ash`.

Current limitations:

- Single-sample swapchain rendering only.
- One frame in flight.
- Backdrop-sampling custom shaders and app-owned textures are not
  implemented in `aetna-ash` yet.

Run:

```sh
cargo run -p aetna-ash-demo --bin hello
cargo run -p aetna-ash-demo --bin showcase
```
