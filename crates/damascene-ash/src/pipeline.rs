use std::ffi::CString;

use ash::vk;
use bytemuck::{Pod, Zeroable};
use damascene_core::paint::QuadInstance;

use crate::naga_compile::wgsl_to_spirv;
use crate::runner::{Error, Result, TargetInfo};

/// Per-frame globals at `@group(0) @binding(0)`. Byte-compatible with
/// the wgpu/vulkano backends.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug, Default)]
pub(crate) struct FrameUniforms {
    pub viewport: [f32; 2],
    pub time: f32,
    pub scale_factor: f32,
}

pub(crate) fn build_quad_pipeline(
    device: &ash::Device,
    pipeline_layout: vk::PipelineLayout,
    target: TargetInfo,
    name: &str,
    wgsl: &str,
) -> Result<vk::Pipeline> {
    let words = wgsl_to_spirv(name, wgsl)?;
    build_quad_pipeline_from_spirv(device, pipeline_layout, target, name, &words)
}

pub(crate) fn build_quad_pipeline_from_spirv(
    device: &ash::Device,
    pipeline_layout: vk::PipelineLayout,
    target: TargetInfo,
    name: &str,
    words: &[u32],
) -> Result<vk::Pipeline> {
    let shader_info = vk::ShaderModuleCreateInfo::default().code(words);
    let shader = unsafe { device.create_shader_module(&shader_info, None) }?;

    let result = build_pipeline_with_module(device, pipeline_layout, target, shader, name);

    unsafe {
        device.destroy_shader_module(shader, None);
    }

    result
}

fn build_pipeline_with_module(
    device: &ash::Device,
    pipeline_layout: vk::PipelineLayout,
    target: TargetInfo,
    shader: vk::ShaderModule,
    name: &str,
) -> Result<vk::Pipeline> {
    let vs_main = CString::new("vs_main").expect("static string has no nul");
    let fs_main = CString::new("fs_main").expect("static string has no nul");
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(shader)
            .name(&vs_main),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(shader)
            .name(&fs_main),
    ];

    let bindings = [
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: (2 * std::mem::size_of::<f32>()) as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        },
        vk::VertexInputBindingDescription {
            binding: 1,
            stride: std::mem::size_of::<QuadInstance>() as u32,
            input_rate: vk::VertexInputRate::INSTANCE,
        },
    ];
    let attrs = [
        attr(0, 0, 0, vk::Format::R32G32_SFLOAT),
        attr(1, 1, 0, vk::Format::R32G32B32A32_SFLOAT),
        attr(2, 1, 16, vk::Format::R32G32B32A32_SFLOAT),
        attr(3, 1, 32, vk::Format::R32G32B32A32_SFLOAT),
        attr(4, 1, 48, vk::Format::R32G32B32A32_SFLOAT),
        attr(5, 1, 64, vk::Format::R32G32B32A32_SFLOAT),
        attr(6, 1, 80, vk::Format::R32G32B32A32_SFLOAT),
        attr(7, 1, 96, vk::Format::R32G32B32A32_SFLOAT),
    ];
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(&bindings)
        .vertex_attribute_descriptions(&attrs);
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_STRIP);
    let viewport_state = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::empty())
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(target.sample_count)
        .sample_shading_enable(target.sample_count != vk::SampleCountFlags::TYPE_1)
        .min_sample_shading(1.0);
    let blend_attachment = vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(true)
        .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .alpha_blend_op(vk::BlendOp::ADD)
        .color_write_mask(
            vk::ColorComponentFlags::R
                | vk::ColorComponentFlags::G
                | vk::ColorComponentFlags::B
                | vk::ColorComponentFlags::A,
        );
    let blend_attachments = [blend_attachment];
    let color_blend =
        vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachments);
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic_state =
        vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
    let color_formats = [target.format];
    let mut rendering =
        vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&color_formats);

    let pipeline_info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport_state)
        .rasterization_state(&rasterization)
        .multisample_state(&multisample)
        .color_blend_state(&color_blend)
        .dynamic_state(&dynamic_state)
        .layout(pipeline_layout)
        .push_next(&mut rendering);

    let pipelines = unsafe {
        device.create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
    }
    .map_err(|(_pipelines, err)| Error::Vulkan {
        op: "create_graphics_pipelines",
        result: err,
    })?;

    pipelines
        .into_iter()
        .next()
        .ok_or(Error::PipelineCreationReturnedEmpty {
            name: name.to_string(),
        })
}

fn attr(
    location: u32,
    binding: u32,
    offset: u32,
    format: vk::Format,
) -> vk::VertexInputAttributeDescription {
    vk::VertexInputAttributeDescription {
        location,
        binding,
        format,
        offset,
    }
}
