//! Low-level `ash`/Vulkan renderer adapter for custom Damascene hosts.
//!
//! Use this crate when your application already owns an `ash` renderer
//! and wants to draw Damascene UI into its frame graph. The host remains
//! responsible for the Vulkan instance, physical device selection,
//! logical device, queues, command pools, command buffers, synchronization,
//! swapchain/surfaces, frame pacing, and platform event loop.
//!
//! The public entry point is [`Runner`]. Its surface intentionally
//! mirrors `damascene-wgpu` / `damascene-vulkano` where ash allows it: Damascene
//! owns interaction state and draw-op preparation; the host owns Vulkan
//! frame management.
//!
//! Rendering support is intentionally staged. Stock quad shaders, text,
//! raster images, flat MSDF icon/vector masks, and tessellated/painted
//! vectors are wired, and host-owned Vulkan textures can be composited
//! through [`app_texture`]. Backdrop sampling remains unsupported.

mod buffer;
mod icon;
mod image;
mod naga_compile;
mod pipeline;
mod runner;
mod scene;
mod surface;
mod text;

pub use naga_compile::{CompileError, wgsl_to_spirv};
pub use runner::{
    AshContext, AshRenderTarget, Error, LoadOp, PreparedFrame, Result, Runner, TargetInfo,
};
pub use surface::{AshAppTexture, app_texture, app_texture_with_layout};

pub use damascene_core::paint::PaintItem;
pub use damascene_core::runtime::{LayoutPrepared, PointerMove, PrepareResult, PrepareTimings};

use ash::vk;

/// Vulkan device features the ash runner's stock pipelines depend on.
///
/// Hosts should merge this into their own feature chain before creating
/// the logical device. For now this matches `damascene-vulkano`: sample-rate
/// shading is required once MSAA stock pipelines are enabled.
pub fn required_device_features() -> vk::PhysicalDeviceFeatures {
    vk::PhysicalDeviceFeatures {
        sample_rate_shading: vk::TRUE,
        ..Default::default()
    }
}

/// Vulkan 1.3 feature-chain requirements for the ash backend.
///
/// `damascene-ash`'s first rendering path uses dynamic rendering rather
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
/// `damascene-ash` records rendering commands into host-owned command
/// buffers and does not create or present swapchains, so it has no
/// mandatory device extensions of its own. A Wayland/winit client host
/// usually still enables `VK_KHR_swapchain` for presentation.
pub fn required_device_extensions() -> &'static [*const std::ffi::c_char] {
    &[]
}
