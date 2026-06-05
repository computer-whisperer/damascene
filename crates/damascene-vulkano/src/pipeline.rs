//! Render pipeline construction for the shared rect-shaped layout.
//!
//! Mirrors `damascene_wgpu::pipeline`: one pipeline-builder factory covers
//! `stock::rounded_rect` and any custom shader the host registers.
//! Every such pipeline reads the same `QuadInstance` ABI (`rect`,
//! `inner_rect`, `vec_a`, `vec_b`, `vec_c`, `vec_d`) and the same
//! `FrameUniforms` at @group(0) @binding(0). Custom shaders that only
//! consume the first four locations still bind correctly; the
//! `inner_rect` and `vec_d` slots are simply unread.
//!
//! The vulkano-side wrinkle is that pipelines are tied to a render-pass
//! subpass at construction time. The Runner owns one render pass (single
//! color attachment when MSAA is off, multisampled color + single-sample
//! resolve when MSAA is on) and every pipeline is built against subpass
//! 0 of that pass with a matching [`multisample_state`].

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use vulkano::{
    device::Device,
    format::Format,
    image::SampleCount,
    pipeline::{
        DynamicState, GraphicsPipeline, PipelineLayout, PipelineShaderStageCreateInfo,
        graphics::{
            GraphicsPipelineCreateInfo,
            color_blend::{AttachmentBlend, ColorBlendAttachmentState, ColorBlendState},
            input_assembly::{InputAssemblyState, PrimitiveTopology},
            multisample::MultisampleState,
            rasterization::RasterizationState,
            subpass::PipelineSubpassType,
            vertex_input::{
                VertexInputAttributeDescription, VertexInputBindingDescription, VertexInputRate,
                VertexInputState,
            },
            viewport::ViewportState,
        },
        layout::PipelineDescriptorSetLayoutCreateInfo,
    },
    render_pass::Subpass,
    shader::{ShaderModule, ShaderModuleCreateInfo, ShaderStages},
};

use damascene_core::paint::QuadInstance;

use crate::naga_compile::wgsl_to_spirv;

/// Per-frame globals at @group(0) @binding(0). Mirrors
/// `damascene_wgpu::pipeline::FrameUniforms` byte-for-byte so the same WGSL
/// reads it identically through both backends.
// `BufferContents` is blanket-implemented for any `bytemuck::AnyBitPattern + Send + Sync`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug, Default)]
pub(crate) struct FrameUniforms {
    pub viewport: [f32; 2],
    pub time: f32,
    pub scale_factor: f32,
    /// Output white-level scale (1.0 on SDR targets); stock fragment
    /// shaders multiply final rgb by it. See docs/COLOR_MANAGEMENT.md.
    pub white_scale: f32,
    /// Reserved — keeps the buffer a 16-byte multiple.
    pub _reserved: [f32; 3],
}

/// Vertex layout shared by every rect-shaped pipeline.
///
/// Binding 0 = the unit-quad corner UVs (4 vertices, `[f32; 2]` each,
/// drawn as a triangle strip). Binding 1 = the instance buffer of
/// `QuadInstance` (7 × `vec4<f32>` per instance at locations 1..=7).
fn vertex_input_state() -> VertexInputState {
    let bind_vertex = VertexInputBindingDescription {
        stride: (2 * std::mem::size_of::<f32>()) as u32,
        input_rate: VertexInputRate::Vertex,
        ..Default::default()
    };
    let bind_instance = VertexInputBindingDescription {
        stride: std::mem::size_of::<QuadInstance>() as u32,
        input_rate: VertexInputRate::Instance { divisor: 1 },
        ..Default::default()
    };
    let attr = |binding: u32, offset: u32, format: Format| VertexInputAttributeDescription {
        binding,
        offset,
        format,
        ..Default::default()
    };

    VertexInputState::new()
        .binding(0, bind_vertex)
        .binding(1, bind_instance)
        // location 0 — corner_uv (binding 0)
        .attribute(0, attr(0, 0, Format::R32G32_SFLOAT))
        // location 1 — rect (binding 1, offset 0) — painted rect
        .attribute(1, attr(1, 0, Format::R32G32B32A32_SFLOAT))
        // location 2 — vec_a / fill (binding 1, offset 16)
        .attribute(2, attr(1, 16, Format::R32G32B32A32_SFLOAT))
        // location 3 — vec_b / stroke (binding 1, offset 32)
        .attribute(3, attr(1, 32, Format::R32G32B32A32_SFLOAT))
        // location 4 — vec_c / params (binding 1, offset 48)
        .attribute(4, attr(1, 48, Format::R32G32B32A32_SFLOAT))
        // location 5 — inner_rect (binding 1, offset 64) — layout rect (NEW)
        .attribute(5, attr(1, 64, Format::R32G32B32A32_SFLOAT))
        // location 6 — vec_d / focus_color (binding 1, offset 80)
        .attribute(6, attr(1, 80, Format::R32G32B32A32_SFLOAT))
        // location 7 — vec_e / per-corner radii (binding 1, offset 96)
        .attribute(7, attr(1, 96, Format::R32G32B32A32_SFLOAT))
}

/// Build a pipeline layout from reflection, then broaden every set-0
/// binding to be visible from both vertex and fragment stages.
///
/// Reflection-derived stage flags differ across our shaders: stock
/// `rounded_rect` and `text` read `frame.viewport` only in the vertex
/// stage, while `liquid_glass` reads `frame.time` in the fragment
/// stage. That gives them non-identical set-0 layouts (`VERTEX` vs
/// `VERTEX | FRAGMENT`), and the runner's per-frame `frame_descriptor_set`
/// (rebuilt against a cached set-0 layout each `prepare()`) is
/// incompatible with whichever pipeline was built later (Vulkan
/// VUID-vkCmdBindDescriptorSets-pDescriptorSets-00358).
///
/// Forcing every set-0 binding to `VERTEX | FRAGMENT` produces a
/// structurally-identical set-0 layout across all pipelines, so the
/// shared frame descriptor set binds correctly into all of them. Set 1
/// (backdrop / atlas) is left at whatever the reflection produced —
/// those layouts are per-shader-family already and don't need to match
/// across stock and custom pipelines.
pub(crate) fn build_shared_pipeline_layout(
    device: Arc<Device>,
    stages: &[PipelineShaderStageCreateInfo],
) -> Arc<PipelineLayout> {
    let mut info = PipelineDescriptorSetLayoutCreateInfo::from_stages(stages);
    if let Some(set0) = info.set_layouts.get_mut(0) {
        for binding in set0.bindings.values_mut() {
            binding.stages |= ShaderStages::VERTEX | ShaderStages::FRAGMENT;
        }
    }
    PipelineLayout::new(
        device.clone(),
        info.into_pipeline_layout_create_info(device)
            .expect("damascene-vulkano: pipeline layout from stages"),
    )
    .expect("damascene-vulkano: pipeline layout new")
}

/// Multisample state for a pipeline drawing into a render pass with
/// `sample_count` rasterization samples. Mirrors the wgpu side's
/// `per_sample_shading: true` default: when MSAA is on we ask the driver
/// to evaluate the fragment shader once per sample so
/// `@interpolate(perspective, sample)` qualifiers (`stock::rounded_rect`'s
/// quad-AA, vector relief/glass shading) sample at their declared
/// frequency instead of being silently downgraded to pixel-rate.
pub(crate) fn multisample_state(sample_count: u32) -> MultisampleState {
    let rasterization_samples = SampleCount::try_from(sample_count).unwrap_or(SampleCount::Sample1);
    MultisampleState {
        rasterization_samples,
        sample_shading: if sample_count > 1 { Some(1.0) } else { None },
        ..Default::default()
    }
}

/// Compile WGSL → SPIR-V and build a graphics pipeline against the
/// shared rect-shaped vertex layout, alpha blending, and the given
/// render-pass subpass. Panics if the WGSL fails to compile.
pub(crate) fn build_quad_pipeline(
    device: Arc<Device>,
    subpass: Subpass,
    sample_count: u32,
    name: &str,
    wgsl: &str,
) -> Arc<GraphicsPipeline> {
    let words = wgsl_to_spirv(name, wgsl).unwrap_or_else(|e| panic!("WGSL compile failed: {e}"));
    // SAFETY: the SPIR-V words are the verified output of naga's spv-out
    // emitter; they passed `naga::valid::Validator` before reaching us.
    let module = unsafe {
        ShaderModule::new(device.clone(), ShaderModuleCreateInfo::new(&words))
            .unwrap_or_else(|e| panic!("ShaderModule::new for `{name}`: {e}"))
    };

    let vs = module
        .entry_point("vs_main")
        .unwrap_or_else(|| panic!("`{name}` has no `vs_main` entry point"));
    let fs = module
        .entry_point("fs_main")
        .unwrap_or_else(|| panic!("`{name}` has no `fs_main` entry point"));

    let stages = [
        PipelineShaderStageCreateInfo::new(vs),
        PipelineShaderStageCreateInfo::new(fs),
    ];

    let layout = build_shared_pipeline_layout(device.clone(), &stages);

    GraphicsPipeline::new(
        device.clone(),
        None,
        GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(vertex_input_state()),
            input_assembly_state: Some(InputAssemblyState {
                topology: PrimitiveTopology::TriangleStrip,
                ..Default::default()
            }),
            viewport_state: Some(ViewportState::default()),
            rasterization_state: Some(RasterizationState::default()),
            multisample_state: Some(multisample_state(sample_count)),
            color_blend_state: Some(ColorBlendState::with_attachment_states(
                subpass.num_color_attachments(),
                ColorBlendAttachmentState {
                    blend: Some(AttachmentBlend::alpha()),
                    ..Default::default()
                },
            )),
            dynamic_state: [DynamicState::Viewport, DynamicState::Scissor]
                .into_iter()
                .collect(),
            subpass: Some(PipelineSubpassType::BeginRenderPass(subpass)),
            ..GraphicsPipelineCreateInfo::layout(layout)
        },
    )
    .unwrap_or_else(|e| panic!("GraphicsPipeline::new for `{name}`: {e:?}"))
}
