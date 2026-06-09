use std::ffi::CString;
use std::ops::Range;

use ash::vk;
use bytemuck::{Pod, Zeroable};
use cosmic_text::fontdb;
use damascene_core::color::ColorSpace;
use damascene_core::ir::TextAnchor;
use damascene_core::paint::{DEFAULT_WORKING_COLOR_SPACE, PhysicalScissor, rgba_f32_in};
use damascene_core::runtime::TextRecorder;
use damascene_core::shader::stock_wgsl;
use damascene_core::text::atlas::{
    ATLAS_BYTES_PER_PIXEL, AtlasPage, AtlasRect, GlyphAtlas, GlyphSlot, RunStyle, ShapedGlyph,
    ShapedRun,
};
use damascene_core::text::msdf_atlas::{
    DEFAULT_BASE_EM, DEFAULT_SPREAD, MsdfAtlas, MsdfAtlasPage, MsdfGlyphKey, MsdfRect, MsdfSlot,
};
use damascene_core::tree::{Rect, TextWrap};
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::Allocator;
use ttf_parser::Face;

use crate::buffer::{GpuBuffer, GpuImage};
use crate::naga_compile::wgsl_to_spirv;
use crate::runner::{Error, Result, TargetInfo};

const INITIAL_TEXT_INSTANCE_CAPACITY: usize = 1024;
const MAX_RETIRED_UPLOADS: usize = 128;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct ColorGlyphInstance {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct MsdfGlyphInstance {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: [f32; 4],
    pub params: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct HighlightInstance {
    pub rect: [f32; 4],
    pub color: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextRunKind {
    Color,
    Msdf,
    Highlight,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TextRun {
    pub kind: TextRunKind,
    pub page: u32,
    pub scissor: Option<PhysicalScissor>,
    pub first: u32,
    pub count: u32,
}

struct PageGpu {
    image: GpuImage,
    descriptor_set: vk::DescriptorSet,
    layout: vk::ImageLayout,
}

struct PendingUpload {
    kind: TextRunKind,
    page: usize,
    rect: [u32; 4],
    staging: GpuBuffer,
}

pub(crate) struct TextPaint {
    atlas: GlyphAtlas,
    msdf_atlas: MsdfAtlas,
    atlas_set_layout: vk::DescriptorSetLayout,
    color_pipeline_layout: vk::PipelineLayout,
    msdf_pipeline_layout: vk::PipelineLayout,
    highlight_pipeline_layout: vk::PipelineLayout,
    color_pipeline: vk::Pipeline,
    msdf_pipeline: vk::Pipeline,
    highlight_pipeline: vk::Pipeline,
    descriptor_pool: vk::DescriptorPool,
    sampler: vk::Sampler,
    color_pages: Vec<PageGpu>,
    msdf_pages: Vec<PageGpu>,
    color_instances: Vec<ColorGlyphInstance>,
    msdf_instances: Vec<MsdfGlyphInstance>,
    highlight_instances: Vec<HighlightInstance>,
    color_instance_buf: GpuBuffer,
    msdf_instance_buf: GpuBuffer,
    highlight_instance_buf: GpuBuffer,
    color_capacity: usize,
    msdf_capacity: usize,
    highlight_capacity: usize,
    runs: Vec<TextRun>,
    pending_uploads: Vec<PendingUpload>,
    retired_uploads: Vec<GpuBuffer>,
    /// Working color space glyph/highlight/decoration colors are
    /// converted into at record time. Kept in sync with the owning
    /// `Runner` via `set_working_color_space`.
    working_color_space: ColorSpace,
}

impl TextPaint {
    pub(crate) fn new(
        device: &ash::Device,
        allocator: &mut Allocator,
        frame_set_layout: vk::DescriptorSetLayout,
        target: TargetInfo,
    ) -> Result<Self> {
        let atlas_set_layout = create_atlas_set_layout(device)?;
        let color_pipeline_layout =
            create_pipeline_layout(device, &[frame_set_layout, atlas_set_layout])?;
        let msdf_pipeline_layout =
            create_pipeline_layout(device, &[frame_set_layout, atlas_set_layout])?;
        let highlight_pipeline_layout = create_pipeline_layout(device, &[frame_set_layout])?;
        let color_pipeline = build_text_pipeline(
            device,
            color_pipeline_layout,
            target,
            "stock::text",
            stock_wgsl::TEXT,
            TextVertexKind::Color,
        )?;
        let msdf_pipeline = build_text_pipeline(
            device,
            msdf_pipeline_layout,
            target,
            "stock::text_msdf",
            stock_wgsl::TEXT_MSDF,
            TextVertexKind::Msdf,
        )?;
        let highlight_pipeline = build_text_pipeline(
            device,
            highlight_pipeline_layout,
            target,
            "stock::text_highlight",
            stock_wgsl::TEXT_HIGHLIGHT,
            TextVertexKind::Highlight,
        )?;
        let pool_sizes = [
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLED_IMAGE,
                descriptor_count: 256,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLER,
                descriptor_count: 256,
            },
        ];
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .max_sets(256)
            .pool_sizes(&pool_sizes);
        let descriptor_pool = unsafe { device.create_descriptor_pool(&pool_info, None) }?;
        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE);
        let sampler = unsafe { device.create_sampler(&sampler_info, None) }?;

        let color_capacity = INITIAL_TEXT_INSTANCE_CAPACITY;
        let msdf_capacity = INITIAL_TEXT_INSTANCE_CAPACITY;
        let highlight_capacity = INITIAL_TEXT_INSTANCE_CAPACITY;
        let color_instance_buf = GpuBuffer::new(
            device,
            allocator,
            "damascene_ash::text_color_instances",
            (color_capacity * std::mem::size_of::<ColorGlyphInstance>()) as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        let msdf_instance_buf = GpuBuffer::new(
            device,
            allocator,
            "damascene_ash::text_msdf_instances",
            (msdf_capacity * std::mem::size_of::<MsdfGlyphInstance>()) as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        let highlight_instance_buf = GpuBuffer::new(
            device,
            allocator,
            "damascene_ash::text_highlight_instances",
            (highlight_capacity * std::mem::size_of::<HighlightInstance>()) as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;

        Ok(Self {
            atlas: GlyphAtlas::new(),
            msdf_atlas: MsdfAtlas::new(DEFAULT_BASE_EM, DEFAULT_SPREAD),
            atlas_set_layout,
            color_pipeline_layout,
            msdf_pipeline_layout,
            highlight_pipeline_layout,
            color_pipeline,
            msdf_pipeline,
            highlight_pipeline,
            descriptor_pool,
            sampler,
            color_pages: Vec::new(),
            msdf_pages: Vec::new(),
            color_instances: Vec::new(),
            msdf_instances: Vec::new(),
            highlight_instances: Vec::new(),
            color_instance_buf,
            msdf_instance_buf,
            highlight_instance_buf,
            color_capacity,
            msdf_capacity,
            highlight_capacity,
            runs: Vec::new(),
            pending_uploads: Vec::new(),
            retired_uploads: Vec::new(),
            working_color_space: DEFAULT_WORKING_COLOR_SPACE,
        })
    }

    /// Update the working color space subsequent glyph/highlight color
    /// packing converts into. Called by `Runner::set_working_color_space`.
    pub(crate) fn set_working_color_space(&mut self, space: ColorSpace) {
        self.working_color_space = space;
    }

    pub(crate) fn frame_begin(&mut self) {
        self.color_instances.clear();
        self.msdf_instances.clear();
        self.highlight_instances.clear();
        self.runs.clear();
    }

    pub(crate) fn flush(&mut self, device: &ash::Device, allocator: &mut Allocator) -> Result<()> {
        while self.retired_uploads.len() > MAX_RETIRED_UPLOADS {
            let mut staging = self.retired_uploads.remove(0);
            unsafe {
                staging.destroy(device, allocator);
            }
        }

        let color_dirty = self.atlas.take_dirty();
        while self.color_pages.len() < self.atlas.pages().len() {
            let i = self.color_pages.len();
            let page = &self.atlas.pages()[i];
            let new_page = self.create_page(
                device,
                allocator,
                vk::Format::R8G8B8A8_SRGB,
                page.width,
                page.height,
            )?;
            self.color_pages.push(new_page);
        }

        let msdf_dirty = self.msdf_atlas.take_dirty();
        while self.msdf_pages.len() < self.msdf_atlas.pages().len() {
            let i = self.msdf_pages.len();
            let page = &self.msdf_atlas.pages()[i];
            let new_page = self.create_page(
                device,
                allocator,
                vk::Format::R8G8B8A8_UNORM,
                page.width,
                page.height,
            )?;
            self.msdf_pages.push(new_page);
        }

        for (page_idx, rect) in color_dirty {
            if rect.w == 0 || rect.h == 0 {
                continue;
            }
            let page = &self.atlas.pages()[page_idx];
            let bytes = pack_color_rect_bytes(page, rect);
            self.queue_upload(
                device,
                allocator,
                TextRunKind::Color,
                page_idx,
                [rect.x, rect.y, rect.w, rect.h],
                &bytes,
            )?;
        }
        for (page_idx, rect) in msdf_dirty {
            if rect.w == 0 || rect.h == 0 {
                continue;
            }
            let page = &self.msdf_atlas.pages()[page_idx];
            let bytes = pack_msdf_rect_bytes(page, rect);
            self.queue_upload(
                device,
                allocator,
                TextRunKind::Msdf,
                page_idx,
                [rect.x, rect.y, rect.w, rect.h],
                &bytes,
            )?;
        }

        self.ensure_buffer_capacity(device, allocator, TextRunKind::Color)?;
        self.ensure_buffer_capacity(device, allocator, TextRunKind::Msdf)?;
        self.ensure_buffer_capacity(device, allocator, TextRunKind::Highlight)?;
        self.color_instance_buf
            .write_bytes(bytemuck::cast_slice(&self.color_instances))?;
        self.msdf_instance_buf
            .write_bytes(bytemuck::cast_slice(&self.msdf_instances))?;
        self.highlight_instance_buf
            .write_bytes(bytemuck::cast_slice(&self.highlight_instances))?;
        Ok(())
    }

    unsafe fn record_uploads(
        &mut self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
    ) -> Result<()> {
        for upload in self.pending_uploads.drain(..) {
            let page = match upload.kind {
                TextRunKind::Color => &mut self.color_pages[upload.page],
                TextRunKind::Msdf => &mut self.msdf_pages[upload.page],
                TextRunKind::Highlight => unreachable!("highlight uploads do not exist"),
            };
            unsafe {
                transition_image(
                    device,
                    cmd,
                    page.image.image,
                    page.layout,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                );
                let region = vk::BufferImageCopy::default()
                    .image_subresource(
                        vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .layer_count(1),
                    )
                    .image_offset(vk::Offset3D {
                        x: upload.rect[0] as i32,
                        y: upload.rect[1] as i32,
                        z: 0,
                    })
                    .image_extent(vk::Extent3D {
                        width: upload.rect[2],
                        height: upload.rect[3],
                        depth: 1,
                    });
                device.cmd_copy_buffer_to_image(
                    cmd,
                    upload.staging.buffer,
                    page.image.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[region],
                );
                transition_image(
                    device,
                    cmd,
                    page.image.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                );
            }
            page.layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
            self.retired_uploads.push(upload.staging);
        }
        Ok(())
    }

    pub(crate) unsafe fn record_pending_uploads(
        &mut self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
    ) -> Result<()> {
        unsafe { self.record_uploads(device, cmd) }
    }

    fn create_page(
        &self,
        device: &ash::Device,
        allocator: &mut Allocator,
        format: vk::Format,
        width: u32,
        height: u32,
    ) -> Result<PageGpu> {
        let image = GpuImage::new(
            device,
            allocator,
            "damascene_ash::text_atlas_page",
            format,
            vk::Extent2D { width, height },
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
        )?;
        let set_layouts = [self.atlas_set_layout];
        let allocate_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(self.descriptor_pool)
            .set_layouts(&set_layouts);
        let descriptor_set = unsafe { device.allocate_descriptor_sets(&allocate_info) }?[0];
        let image_info = vk::DescriptorImageInfo::default()
            .image_view(image.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
        let sampler_info = vk::DescriptorImageInfo::default().sampler(self.sampler);
        let writes = [
            vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .image_info(std::slice::from_ref(&image_info)),
            vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .image_info(std::slice::from_ref(&sampler_info)),
        ];
        unsafe {
            device.update_descriptor_sets(&writes, &[]);
        }
        Ok(PageGpu {
            image,
            descriptor_set,
            layout: vk::ImageLayout::UNDEFINED,
        })
    }

    fn queue_upload(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        kind: TextRunKind,
        page: usize,
        rect: [u32; 4],
        bytes: &[u8],
    ) -> Result<()> {
        let mut staging = GpuBuffer::new(
            device,
            allocator,
            "damascene_ash::text_atlas_staging",
            bytes.len() as vk::DeviceSize,
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
        )?;
        staging.write_bytes(bytes)?;
        self.pending_uploads.push(PendingUpload {
            kind,
            page,
            rect,
            staging,
        });
        Ok(())
    }

    fn ensure_buffer_capacity(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        kind: TextRunKind,
    ) -> Result<()> {
        let (len, capacity, buffer, name, stride) = match kind {
            TextRunKind::Color => (
                self.color_instances.len(),
                &mut self.color_capacity,
                &mut self.color_instance_buf,
                "damascene_ash::text_color_instances",
                std::mem::size_of::<ColorGlyphInstance>(),
            ),
            TextRunKind::Msdf => (
                self.msdf_instances.len(),
                &mut self.msdf_capacity,
                &mut self.msdf_instance_buf,
                "damascene_ash::text_msdf_instances",
                std::mem::size_of::<MsdfGlyphInstance>(),
            ),
            TextRunKind::Highlight => (
                self.highlight_instances.len(),
                &mut self.highlight_capacity,
                &mut self.highlight_instance_buf,
                "damascene_ash::text_highlight_instances",
                std::mem::size_of::<HighlightInstance>(),
            ),
        };
        if len <= *capacity {
            return Ok(());
        }
        let mut next = (*capacity).max(1);
        while next < len {
            next *= 2;
        }
        unsafe {
            buffer.destroy(device, allocator);
        }
        *buffer = GpuBuffer::new(
            device,
            allocator,
            name,
            (next * stride) as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        *capacity = next;
        Ok(())
    }

    pub(crate) fn run(&self, index: usize) -> TextRun {
        self.runs[index]
    }

    pub(crate) fn pipeline_for(&self, kind: TextRunKind) -> vk::Pipeline {
        match kind {
            TextRunKind::Color => self.color_pipeline,
            TextRunKind::Msdf => self.msdf_pipeline,
            TextRunKind::Highlight => self.highlight_pipeline,
        }
    }

    pub(crate) fn pipeline_layout_for(&self, kind: TextRunKind) -> vk::PipelineLayout {
        match kind {
            TextRunKind::Color => self.color_pipeline_layout,
            TextRunKind::Msdf => self.msdf_pipeline_layout,
            TextRunKind::Highlight => self.highlight_pipeline_layout,
        }
    }

    pub(crate) fn page_descriptor(&self, kind: TextRunKind, page: u32) -> vk::DescriptorSet {
        match kind {
            TextRunKind::Color => self.color_pages[page as usize].descriptor_set,
            TextRunKind::Msdf => self.msdf_pages[page as usize].descriptor_set,
            TextRunKind::Highlight => unreachable!("highlight runs carry no page descriptor"),
        }
    }

    pub(crate) fn instance_buffer(&self, kind: TextRunKind) -> vk::Buffer {
        match kind {
            TextRunKind::Color => self.color_instance_buf.buffer,
            TextRunKind::Msdf => self.msdf_instance_buf.buffer,
            TextRunKind::Highlight => self.highlight_instance_buf.buffer,
        }
    }

    pub(crate) unsafe fn destroy(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        unsafe {
            for mut upload in self.pending_uploads.drain(..) {
                upload.staging.destroy(device, allocator);
            }
            for mut staging in self.retired_uploads.drain(..) {
                staging.destroy(device, allocator);
            }
            for page in &mut self.color_pages {
                page.image.destroy(device, allocator);
            }
            for page in &mut self.msdf_pages {
                page.image.destroy(device, allocator);
            }
            self.color_instance_buf.destroy(device, allocator);
            self.msdf_instance_buf.destroy(device, allocator);
            self.highlight_instance_buf.destroy(device, allocator);
            device.destroy_pipeline(self.color_pipeline, None);
            device.destroy_pipeline(self.msdf_pipeline, None);
            device.destroy_pipeline(self.highlight_pipeline, None);
            device.destroy_pipeline_layout(self.color_pipeline_layout, None);
            device.destroy_pipeline_layout(self.msdf_pipeline_layout, None);
            device.destroy_pipeline_layout(self.highlight_pipeline_layout, None);
            device.destroy_sampler(self.sampler, None);
            device.destroy_descriptor_pool(self.descriptor_pool, None);
            device.destroy_descriptor_set_layout(self.atlas_set_layout, None);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn record_inner(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        runs: &[(String, RunStyle)],
        size: f32,
        line_height: f32,
        wrap: TextWrap,
        anchor: TextAnchor,
        scale_factor: f32,
    ) -> Range<usize> {
        let avail = wrap_available_width(rect.w, wrap, anchor);
        let runs_ref: Vec<(&str, RunStyle)> = runs
            .iter()
            .map(|(text, style)| (text.as_str(), style.clone()))
            .collect();
        let shaped = self.atlas.shape_runs_with_line_height(
            &runs_ref,
            size,
            line_height,
            wrap,
            anchor,
            avail,
        );
        self.emit_shaped_glyphs(rect, scissor, &shaped, wrap, scale_factor)
    }

    fn emit_shaped_glyphs(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        shaped: &ShapedRun,
        wrap: TextWrap,
        scale_factor: f32,
    ) -> Range<usize> {
        let runs_start = self.runs.len();
        if shaped.glyphs.is_empty() && shaped.highlights.is_empty() && shaped.decorations.is_empty()
        {
            return runs_start..runs_start;
        }
        let v_offset = match wrap {
            TextWrap::NoWrap => ((rect.h - shaped.layout.height).max(0.0)) * 0.5,
            TextWrap::Wrap => 0.0,
        };
        let origin_x = rect.x;
        let origin_y = rect.y + v_offset;

        if !shaped.highlights.is_empty() {
            let first = self.highlight_instances.len() as u32;
            for h in &shaped.highlights {
                self.highlight_instances.push(HighlightInstance {
                    rect: [origin_x + h.x, origin_y + h.y, h.w, h.h],
                    color: rgba_f32_in(h.color, self.working_color_space),
                });
            }
            let count = self.highlight_instances.len() as u32 - first;
            if count > 0 {
                self.runs.push(TextRun {
                    kind: TextRunKind::Highlight,
                    page: 0,
                    scissor,
                    first,
                    count,
                });
            }
        }

        let mut current: Option<(TextRunKind, u32, u32)> = None;
        for glyph in &shaped.glyphs {
            let font_id = glyph.key.font;
            if self.atlas.is_color_font(font_id) {
                self.atlas.ensure_color_glyph(glyph.key);
                let Some(slot) = self.atlas.slot(glyph.key) else {
                    continue;
                };
                if slot.rect.w == 0 || slot.rect.h == 0 {
                    continue;
                }
                self.maybe_close_run(&mut current, TextRunKind::Color, slot.page, scissor);
                self.push_color_glyph(glyph, slot, origin_x, origin_y, scale_factor);
            } else {
                let mkey = MsdfGlyphKey {
                    font: font_id,
                    glyph_id: glyph.key.glyph_id,
                };
                let Some(slot) = self.ensure_msdf(mkey, font_id, glyph.key.weight) else {
                    continue;
                };
                self.maybe_close_run(&mut current, TextRunKind::Msdf, slot.page, scissor);
                self.push_msdf_glyph(glyph, slot, origin_x, origin_y);
            }
        }
        if let Some((kind, page, first)) = current {
            let count = self.instance_count_after(kind, first);
            if count > 0 {
                self.runs.push(TextRun {
                    kind,
                    page,
                    scissor,
                    first,
                    count,
                });
            }
        }

        if !shaped.decorations.is_empty() {
            let first = self.highlight_instances.len() as u32;
            for d in &shaped.decorations {
                self.highlight_instances.push(HighlightInstance {
                    rect: [origin_x + d.x, origin_y + d.y, d.w, d.h],
                    color: rgba_f32_in(d.color, self.working_color_space),
                });
            }
            let count = self.highlight_instances.len() as u32 - first;
            if count > 0 {
                self.runs.push(TextRun {
                    kind: TextRunKind::Highlight,
                    page: 0,
                    scissor,
                    first,
                    count,
                });
            }
        }
        runs_start..self.runs.len()
    }

    fn maybe_close_run(
        &mut self,
        current: &mut Option<(TextRunKind, u32, u32)>,
        next_kind: TextRunKind,
        next_page: u32,
        scissor: Option<PhysicalScissor>,
    ) {
        let new_start = match next_kind {
            TextRunKind::Color => self.color_instances.len() as u32,
            TextRunKind::Msdf => self.msdf_instances.len() as u32,
            TextRunKind::Highlight => self.highlight_instances.len() as u32,
        };
        let needs_close = current
            .as_ref()
            .is_some_and(|(kind, page, _)| *kind != next_kind || *page != next_page);
        if needs_close {
            let (kind, page, first) = current.take().unwrap();
            let count = self.instance_count_after(kind, first);
            if count > 0 {
                self.runs.push(TextRun {
                    kind,
                    page,
                    scissor,
                    first,
                    count,
                });
            }
        }
        if current.is_none() {
            *current = Some((next_kind, next_page, new_start));
        }
    }

    fn instance_count_after(&self, kind: TextRunKind, first: u32) -> u32 {
        let len = match kind {
            TextRunKind::Color => self.color_instances.len() as u32,
            TextRunKind::Msdf => self.msdf_instances.len() as u32,
            TextRunKind::Highlight => self.highlight_instances.len() as u32,
        };
        len.saturating_sub(first)
    }

    fn push_color_glyph(
        &mut self,
        glyph: &ShapedGlyph,
        slot: GlyphSlot,
        origin_x: f32,
        origin_y: f32,
        scale_factor: f32,
    ) {
        // The atlas quantizes sizes to whole px (so animated sizes
        // don't mint a bitmap per frame); scale the quad by the
        // requested/rasterized ratio so it renders at the exact
        // requested size.
        let ratio = if slot.raster_size > 0.0 {
            glyph.key.size() / slot.raster_size
        } else {
            1.0
        };
        let bx = origin_x + glyph.x + slot.offset.0 as f32 * ratio / scale_factor;
        let by = origin_y + glyph.y - slot.offset.1 as f32 * ratio / scale_factor;
        let bw = slot.rect.w as f32 * ratio / scale_factor;
        let bh = slot.rect.h as f32 * ratio / scale_factor;
        let atlas_page = self
            .atlas
            .page(slot.page)
            .expect("shaped glyph references missing colour atlas page");
        let page_w = atlas_page.width as f32;
        let page_h = atlas_page.height as f32;
        let uv = [
            slot.rect.x as f32 / page_w,
            slot.rect.y as f32 / page_h,
            slot.rect.w as f32 / page_w,
            slot.rect.h as f32 / page_h,
        ];
        let color = if slot.is_color {
            [1.0, 1.0, 1.0, 1.0]
        } else {
            rgba_f32_in(glyph.color, self.working_color_space)
        };
        self.color_instances.push(ColorGlyphInstance {
            rect: [bx, by, bw, bh],
            uv,
            color,
        });
    }

    fn push_msdf_glyph(
        &mut self,
        glyph: &ShapedGlyph,
        slot: MsdfSlot,
        origin_x: f32,
        origin_y: f32,
    ) {
        let logical_em = glyph.key.size();
        let base_em = self.msdf_atlas.base_em() as f32;
        let scale = logical_em / base_em;
        let bx = origin_x + glyph.x + slot.bearing_x * scale;
        let by = origin_y + glyph.y + slot.bearing_y * scale;
        let bw = slot.rect.w as f32 * scale;
        let bh = slot.rect.h as f32 * scale;
        let atlas_page = self
            .msdf_atlas
            .page(slot.page)
            .expect("shaped glyph references missing MSDF atlas page");
        let page_w = atlas_page.width as f32;
        let page_h = atlas_page.height as f32;
        let uv = [
            slot.rect.x as f32 / page_w,
            slot.rect.y as f32 / page_h,
            slot.rect.w as f32 / page_w,
            slot.rect.h as f32 / page_h,
        ];
        self.msdf_instances.push(MsdfGlyphInstance {
            rect: [bx, by, bw, bh],
            uv,
            color: rgba_f32_in(glyph.color, self.working_color_space),
            params: [slot.spread, 0.0, 0.0, 0.0],
        });
    }

    fn ensure_msdf(
        &mut self,
        key: MsdfGlyphKey,
        font_id: fontdb::ID,
        weight: fontdb::Weight,
    ) -> Option<MsdfSlot> {
        // touch (rather than slot) stamps the page as used this frame
        // so the LRU page recycler skips it.
        if let Some(slot) = self.msdf_atlas.touch(key) {
            return Some(slot);
        }
        let font = self.atlas.font_system_mut().get_font(font_id, weight)?;
        let face_index = self.atlas.font_system().db().face(font_id)?.index;
        let face = Face::parse(font.data(), face_index).ok()?;
        self.msdf_atlas.ensure(key, &face)
    }
}

impl TextRecorder for TextPaint {
    fn record(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        style: &RunStyle,
        text: &str,
        size: f32,
        line_height: f32,
        wrap: TextWrap,
        anchor: TextAnchor,
        scale_factor: f32,
    ) -> Range<usize> {
        self.record_inner(
            rect,
            scissor,
            &[(text.to_string(), style.clone())],
            size,
            line_height,
            wrap,
            anchor,
            scale_factor,
        )
    }

    fn record_runs(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        runs: &[(String, RunStyle)],
        size: f32,
        line_height: f32,
        wrap: TextWrap,
        anchor: TextAnchor,
        scale_factor: f32,
    ) -> Range<usize> {
        self.record_inner(
            rect,
            scissor,
            runs,
            size,
            line_height,
            wrap,
            anchor,
            scale_factor,
        )
    }
}

fn wrap_available_width(rect_w: f32, wrap: TextWrap, anchor: TextAnchor) -> Option<f32> {
    match (wrap, anchor) {
        (TextWrap::Wrap, _) => Some(rect_w),
        (TextWrap::NoWrap, TextAnchor::Start) => None,
        (TextWrap::NoWrap, TextAnchor::Middle | TextAnchor::End) => Some(rect_w),
    }
}

fn pack_color_rect_bytes(page: &AtlasPage, rect: AtlasRect) -> Vec<u8> {
    let bpp = ATLAS_BYTES_PER_PIXEL as usize;
    let row_bytes = rect.w as usize * bpp;
    let mut bytes = Vec::with_capacity(row_bytes * rect.h as usize);
    for row in 0..rect.h {
        let y = rect.y + row;
        let start = (y as usize * page.width as usize + rect.x as usize) * bpp;
        let end = start + row_bytes;
        bytes.extend_from_slice(&page.pixels[start..end]);
    }
    bytes
}

fn pack_msdf_rect_bytes(page: &MsdfAtlasPage, rect: MsdfRect) -> Vec<u8> {
    const BPP: usize = 4;
    let row_bytes = rect.w as usize * BPP;
    let mut bytes = Vec::with_capacity(row_bytes * rect.h as usize);
    for row in 0..rect.h {
        let y = rect.y + row;
        let start = (y as usize * page.width as usize + rect.x as usize) * BPP;
        let end = start + row_bytes;
        bytes.extend_from_slice(&page.pixels[start..end]);
    }
    bytes
}

fn create_atlas_set_layout(device: &ash::Device) -> Result<vk::DescriptorSetLayout> {
    let bindings = [
        vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT),
        vk::DescriptorSetLayoutBinding::default()
            .binding(1)
            .descriptor_type(vk::DescriptorType::SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT),
    ];
    let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
    unsafe { device.create_descriptor_set_layout(&info, None) }.map_err(Into::into)
}

fn create_pipeline_layout(
    device: &ash::Device,
    layouts: &[vk::DescriptorSetLayout],
) -> Result<vk::PipelineLayout> {
    let info = vk::PipelineLayoutCreateInfo::default().set_layouts(layouts);
    unsafe { device.create_pipeline_layout(&info, None) }.map_err(Into::into)
}

#[derive(Clone, Copy)]
enum TextVertexKind {
    Color,
    Msdf,
    Highlight,
}

fn build_text_pipeline(
    device: &ash::Device,
    pipeline_layout: vk::PipelineLayout,
    target: TargetInfo,
    name: &str,
    wgsl: &str,
    kind: TextVertexKind,
) -> Result<vk::Pipeline> {
    let words = wgsl_to_spirv(name, wgsl)?;
    let shader_info = vk::ShaderModuleCreateInfo::default().code(&words);
    let shader = unsafe { device.create_shader_module(&shader_info, None) }?;
    let result =
        build_text_pipeline_with_module(device, pipeline_layout, target, name, shader, kind);
    unsafe {
        device.destroy_shader_module(shader, None);
    }
    result
}

fn build_text_pipeline_with_module(
    device: &ash::Device,
    pipeline_layout: vk::PipelineLayout,
    target: TargetInfo,
    name: &str,
    shader: vk::ShaderModule,
    kind: TextVertexKind,
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

    let instance_stride = match kind {
        TextVertexKind::Color => std::mem::size_of::<ColorGlyphInstance>(),
        TextVertexKind::Msdf => std::mem::size_of::<MsdfGlyphInstance>(),
        TextVertexKind::Highlight => std::mem::size_of::<HighlightInstance>(),
    };
    let bindings = [
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: (2 * std::mem::size_of::<f32>()) as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        },
        vk::VertexInputBindingDescription {
            binding: 1,
            stride: instance_stride as u32,
            input_rate: vk::VertexInputRate::INSTANCE,
        },
    ];
    let attrs: Vec<vk::VertexInputAttributeDescription> = match kind {
        TextVertexKind::Color => vec![
            attr(0, 0, 0, vk::Format::R32G32_SFLOAT),
            attr(1, 1, 0, vk::Format::R32G32B32A32_SFLOAT),
            attr(2, 1, 16, vk::Format::R32G32B32A32_SFLOAT),
            attr(3, 1, 32, vk::Format::R32G32B32A32_SFLOAT),
        ],
        TextVertexKind::Msdf => vec![
            attr(0, 0, 0, vk::Format::R32G32_SFLOAT),
            attr(1, 1, 0, vk::Format::R32G32B32A32_SFLOAT),
            attr(2, 1, 16, vk::Format::R32G32B32A32_SFLOAT),
            attr(3, 1, 32, vk::Format::R32G32B32A32_SFLOAT),
            attr(4, 1, 48, vk::Format::R32G32B32A32_SFLOAT),
        ],
        TextVertexKind::Highlight => vec![
            attr(0, 0, 0, vk::Format::R32G32_SFLOAT),
            attr(1, 1, 0, vk::Format::R32G32B32A32_SFLOAT),
            attr(2, 1, 16, vk::Format::R32G32B32A32_SFLOAT),
        ],
    };

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
        .src_color_blend_factor(vk::BlendFactor::ONE)
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

unsafe fn transition_image(
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    image: vk::Image,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
) {
    if old_layout == new_layout {
        return;
    }
    let (src_access, src_stage) = match old_layout {
        vk::ImageLayout::UNDEFINED => (
            vk::AccessFlags::empty(),
            vk::PipelineStageFlags::TOP_OF_PIPE,
        ),
        vk::ImageLayout::TRANSFER_DST_OPTIMAL => (
            vk::AccessFlags::TRANSFER_WRITE,
            vk::PipelineStageFlags::TRANSFER,
        ),
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL => (
            vk::AccessFlags::SHADER_READ,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
        ),
        _ => (
            vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
            vk::PipelineStageFlags::ALL_COMMANDS,
        ),
    };
    let (dst_access, dst_stage) = match new_layout {
        vk::ImageLayout::TRANSFER_DST_OPTIMAL => (
            vk::AccessFlags::TRANSFER_WRITE,
            vk::PipelineStageFlags::TRANSFER,
        ),
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL => (
            vk::AccessFlags::SHADER_READ,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
        ),
        _ => (
            vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
            vk::PipelineStageFlags::ALL_COMMANDS,
        ),
    };
    let barrier = vk::ImageMemoryBarrier::default()
        .old_layout(old_layout)
        .new_layout(new_layout)
        .src_access_mask(src_access)
        .dst_access_mask(dst_access)
        .image(image)
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .level_count(1)
                .layer_count(1),
        );
    unsafe {
        device.cmd_pipeline_barrier(
            cmd,
            src_stage,
            dst_stage,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier],
        );
    }
}
