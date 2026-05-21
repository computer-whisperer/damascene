//! Low-level `ash`/Vulkan renderer adapter for custom Aetna hosts.
//!
//! Use this crate when your application already owns an `ash` renderer
//! and wants to draw Aetna UI into its frame graph. The host remains
//! responsible for the Vulkan instance, physical device selection,
//! logical device, queues, command pools, command buffers, synchronization,
//! swapchain/surfaces, frame pacing, and platform event loop.
//!
//! The public entry point is [`Runner`]. Its surface intentionally
//! mirrors `aetna-wgpu` / `aetna-vulkano` where ash allows it: Aetna
//! owns interaction state and draw-op preparation; the host owns Vulkan
//! frame management.
//!
//! This first crate slice establishes the host contract and core runtime
//! forwarding. The Vulkan draw implementation is intentionally staged:
//! [`Runner::draw`] and [`Runner::render`] currently return
//! [`Error::Unsupported`] until the ash pipeline/buffer/descriptor
//! modules are filled in.

mod buffer;
mod naga_compile;
mod pipeline;
mod runner;

pub use naga_compile::{CompileError, wgsl_to_spirv};
pub use runner::{
    AshContext, AshRenderTarget, Error, LoadOp, PreparedFrame, Result, Runner, TargetInfo,
};

pub use aetna_core::paint::PaintItem;
pub use aetna_core::runtime::{LayoutPrepared, PointerMove, PrepareResult, PrepareTimings};

use ash::vk;

/// Vulkan device features the ash runner's stock pipelines depend on.
///
/// Hosts should merge this into their own feature chain before creating
/// the logical device. For now this matches `aetna-vulkano`: sample-rate
/// shading is required once MSAA stock pipelines are enabled.
pub fn required_device_features() -> vk::PhysicalDeviceFeatures {
    vk::PhysicalDeviceFeatures {
        sample_rate_shading: vk::TRUE,
        ..Default::default()
    }
}

/// Vulkan 1.3 feature-chain requirements for the ash backend.
///
/// `aetna-ash`'s first rendering path uses dynamic rendering rather
/// than host-created render passes/framebuffers, so hosts creating a
/// Vulkan 1.3 device should include this in their `p_next` feature
/// chain. Hosts targeting older device APIs will need the equivalent
/// `VK_KHR_dynamic_rendering` extension path, which this crate does not
/// wrap yet.
pub fn required_vulkan_13_features() -> vk::PhysicalDeviceVulkan13Features<'static> {
    vk::PhysicalDeviceVulkan13Features::default().dynamic_rendering(true)
}

/// Device extensions required by the renderer itself.
///
/// `aetna-ash` records rendering commands into host-owned command
/// buffers and does not create or present swapchains, so it has no
/// mandatory device extensions of its own. A Wayland/winit client host
/// usually still enables `VK_KHR_swapchain` for presentation.
pub fn required_device_extensions() -> &'static [*const std::ffi::c_char] {
    &[]
}
