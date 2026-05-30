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
/// };
/// ```
/// Custom shaders that previously declared `_pad: vec2<f32>` keep
/// working — the byte layout is unchanged; the trailing `_pad.y` slot
/// is now `scale_factor` and shaders can either ignore it or rename
/// the field to consume it.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct FrameUniforms {
    pub viewport: [f32; 2],
    pub time: f32,
    pub scale_factor: f32,
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
