//! MSAA color-attachment helper.
//!
//! Damascene's stock surfaces (rounded_rect, gradient, custom rect-shaped
//! shaders) compute coverage analytically inside the fragment shader,
//! so MSAA only helps when sample-rate shading is on — the SDF input
//! varying carries `@interpolate(perspective, sample)` and the shader
//! evaluates per sub-sample. Pair that with a [`Runner`](crate::Runner)
//! built via [`Runner::with_sample_count`](crate::Runner::with_sample_count)
//! and an [`MsaaTarget`] of matching `sample_count` for the attachment.
//!
//! The host always supplies the single-sample resolve target itself —
//! a fresh texture in headless render binaries (so it can be read back
//! / sampled), or the surface frame in windowed apps. `MsaaTarget` is
//! deliberately just the multisampled side; bundling the resolve here
//! would waste a fullscreen texture in the surface case.

/// A multisampled color attachment.
///
/// Hosts allocate one of these to match their resolve target's extent
/// and pass [`Self::view`] as the render pass `view` while binding
/// their resolve target as `resolve_target`. On window resize, check
/// [`Self::matches`] and reallocate when it returns false.
pub struct MsaaTarget {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sample_count: u32,
    pub format: wgpu::TextureFormat,
    pub size: wgpu::Extent3d,
}

impl MsaaTarget {
    /// Build a fresh multisampled color attachment sized to `size`.
    /// Usage is `RENDER_ATTACHMENT` only — multisampled textures cannot
    /// be sampled or copied directly; the resolve target is what the
    /// host reads back, presents, or samples.
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: wgpu::Extent3d,
        sample_count: u32,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("damascene_wgpu::msaa_target"),
            size,
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            sample_count,
            format,
            size,
        }
    }

    /// True when this target's extent matches the requested size — the
    /// idiomatic "do I need to reallocate after a resize?" check.
    pub fn matches(&self, size: wgpu::Extent3d) -> bool {
        self.size.width == size.width
            && self.size.height == size.height
            && self.size.depth_or_array_layers == size.depth_or_array_layers
    }
}
