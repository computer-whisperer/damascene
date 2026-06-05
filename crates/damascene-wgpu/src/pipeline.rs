//! Render pipeline construction for the shared rect-shaped layout.
//!
//! Stock `rounded_rect` and any user-registered custom shader all use
//! the same vertex layout — a unit-quad strip plus the
//! [`damascene_core::paint::QuadInstance`] attributes. That means one
//! pipeline-builder function covers the whole catalog; the only thing
//! that varies is the WGSL source and a label. Focus indicators ride
//! on each focusable node's own quad via uniforms on `rounded_rect` —
//! no separate ring pipeline.

use std::borrow::Cow;

use bytemuck::{Pod, Zeroable};

use damascene_core::paint::QuadInstance;

/// Per-frame globals bound at @group(0).
///
/// Layout matches the shared WGSL convention:
/// ```wgsl
/// struct FrameUniforms {
///     viewport:     vec2<f32>,  // logical px (width, height)
///     time:         f32,        // seconds since runner start
///     scale_factor: f32,        // physical px per logical px (1, 1.5, 2…)
///     white_scale:  f32,        // output white-level scale (1.0 on SDR)
///     headroom:     f32,        // output luminance headroom (1.0 on SDR)
///     ref_nits:     f32,        // output reference white, cd/m²
/// };
/// ```
/// Custom shaders that declare a shorter prefix (the legacy
/// `viewport + _pad: vec2<f32>` 16-byte form, the 16-byte
/// `time`/`scale_factor` form, or the 20-byte `white_scale` form) keep
/// working — the buffer is bound whole and field offsets are
/// unchanged; shaders only declare the prefix they consume.
///
/// `white_scale` lifts working-space content to the output's
/// reference-white level on extended-range (scRGB) surfaces — every
/// stock fragment shader multiplies its final rgb by it. 1.0 on SDR
/// targets. Custom shaders should do the same to their *authored*
/// output; backdrop samples (`@group(1)` snapshot) are already in
/// output-scaled space and must not be scaled again. See
/// docs/COLOR_MANAGEMENT.md.
///
/// `headroom` is the output's usable luminance range in multiples of
/// reference white (`target_max / reference`; 1.0 on SDR, infinity
/// when the output declared no maximum) and `ref_nits` that reference
/// white in cd/m². `stock::image` uses them to remaster HDR images the
/// panel can't show; custom shaders that author HDR light should keep
/// their output within `headroom` the same way.
///
/// The Rust struct carries trailing reserved padding so the GPU buffer
/// stays a 16-byte multiple (GLES/std140-safe headroom for future
/// fields).
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct FrameUniforms {
    pub viewport: [f32; 2],
    pub time: f32,
    pub scale_factor: f32,
    pub white_scale: f32,
    pub headroom: f32,
    pub ref_nits: f32,
    pub _reserved: f32,
}

/// Per-instance vertex attributes — must match the shared
/// `InstanceInput` struct in `shaders/rounded_rect.wgsl` and any
/// registered custom shader. Order matches `damascene_core::paint::QuadInstance`
/// field order so byte offsets line up. Locations 1..=6 are the
/// legacy slots (custom shaders that only declare 1..=N keep working);
/// location 7 carries per-corner radii.
const INSTANCE_ATTRS: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
    1 => Float32x4,  // rect (xy=topleft px, zw=size px) — painted rect
    2 => Float32x4,  // vec_a (stock::rounded_rect: fill)
    3 => Float32x4,  // vec_b (stock::rounded_rect: stroke)
    4 => Float32x4,  // vec_c (stock::rounded_rect: stroke_width, max_radius, shadow, focus_width)
    5 => Float32x4,  // inner_rect (xy=topleft px, zw=size px) — layout rect
    6 => Float32x4,  // vec_d (stock::rounded_rect: focus_color rgba, alpha eased)
    7 => Float32x4,  // vec_e (stock::rounded_rect: per-corner radii tl, tr, br, bl)
];

pub(crate) fn build_quad_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
    sample_count: u32,
    label: &str,
    wgsl: &str,
    per_sample_shading: bool,
) -> wgpu::RenderPipeline {
    // Several stock shaders (rounded_rect, spinner, skeleton,
    // progress_indeterminate) — and some custom ones like the
    // gradient demo — use `@interpolate(perspective, sample)` to opt
    // into per-sample MSAA shading for cleaner SDF AA on rounded
    // corners. naga validates that qualifier against the adapter's
    // `DownlevelFlags::MULTISAMPLED_SHADING` at module-creation time
    // (regardless of pipeline `sample_count`), and WebGL2 — plus most
    // browser WebGPU adapters — don't expose the flag. Without the
    // downlevel, `create_shader_module` panics before pipeline init
    // on those backends. Strip the `, sample` qualifier when the
    // adapter doesn't advertise the cap: the shader then interpolates
    // at pixel centre instead of per sample, which slightly thickens
    // the AA band on curved edges but otherwise renders correctly.
    // MSAA itself (coverage-based) still functions at
    // `sample_count > 1`. Hosts pass the flag from
    // `adapter.get_downlevel_capabilities().flags`.
    let wgsl = if per_sample_shading {
        Cow::Borrowed(wgsl)
    } else {
        Cow::Owned(wgsl.replace(
            "@interpolate(perspective, sample)",
            "@interpolate(perspective)",
        ))
    };
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(wgsl),
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[
                wgpu::VertexBufferLayout {
                    array_stride: (2 * std::mem::size_of::<f32>()) as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                    }],
                },
                wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<QuadInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &INSTANCE_ATTRS,
                },
            ],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: sample_count,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview_mask: None,
        cache: None,
    })
}
