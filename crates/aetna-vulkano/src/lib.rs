//! Native Vulkan backend for custom Aetna hosts.
//!
//! Most applications should implement `aetna_core::App` and run it
//! through `aetna-winit-wgpu`. Use this crate directly when you are
//! validating backend parity or embedding Aetna into an existing Vulkan
//! renderer built on `vulkano`.
//!
//! The public entry point is [`Runner`]. Its surface mirrors
//! `aetna-wgpu::Runner` where the GPU APIs allow it: the host owns the
//! window, device, queue, swapchain, and event loop; the runner owns
//! Aetna interaction state, layout/draw-op preparation, Vulkan
//! pipelines, text atlas images, and icon rendering.
//!
//! WGSL remains the shader source language. This backend uses [`naga`]
//! to compile WGSL to SPIR-V when building pipelines so custom shader
//! fixtures can be shared with the `wgpu` backend.

mod icon;
mod image;
mod instance;
pub mod naga_compile;
mod pipeline;
pub mod runner;
mod scene;
mod surface;
mod text;

pub use naga_compile::{CompileError, wgsl_to_spirv};
pub use runner::{LayoutPrepared, PointerMove, PrepareResult, PrepareTimings, Runner};
pub use surface::{VulkanoAppTexture, app_texture};

/// Vulkan device features the runner's stock pipelines depend on.
/// Hosts must merge this with their own required features when calling
/// `Device::new(..., DeviceCreateInfo { enabled_features, .. })` —
/// otherwise pipeline construction panics with a SPIR-V validation
/// error like "uses the SPIR-V capability `SampleRateShading`".
///
/// Currently this is just `sample_rate_shading`, used by
/// `stock::rounded_rect`'s `@interpolate(perspective, sample)` to keep
/// quad antialiasing one screen-pixel wide under MSAA. Wgpu's
/// device-creation flow turns this on by default; vulkano's doesn't.
pub fn required_device_features() -> vulkano::device::DeviceFeatures {
    vulkano::device::DeviceFeatures {
        sample_rate_shading: true,
        ..vulkano::device::DeviceFeatures::empty()
    }
}
