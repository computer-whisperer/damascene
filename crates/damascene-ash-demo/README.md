# damascene-ash-demo

Native `ash` smoke harness for `damascene-ash`.

This is intentionally smaller than `damascene-winit-wgpu`: it is a demo and
test harness for hosts that already expect to manage Vulkan directly.
The crate owns a winit window, ash Vulkan 1.3 instance/device/queue,
surface, swapchain, command buffer, simple one-frame synchronization,
and winit input translation. The reusable integration surface remains
`damascene-ash`.

Current limitations:

- Single-sample swapchain rendering only.
- One frame in flight.
- Backdrop-sampling custom shaders are not implemented in `damascene-ash`
  yet.

Run:

```sh
cargo run -p damascene-ash-demo --bin hello
cargo run -p damascene-ash-demo --bin showcase
```
