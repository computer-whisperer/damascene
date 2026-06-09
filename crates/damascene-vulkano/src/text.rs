//! Atlas-backed text rendering for the Vulkano backend.
//!
//! Two paths share one [`TextPaint`]:
//!
//! - **MTSDF outline path** (`stock::text_msdf`): each `(font, glyph)`
//!   pair is rasterised once into a multi-channel + true-SDF atlas via
//!   `MsdfAtlas`, and rendered as one quad per glyph regardless of UI
//!   size. Used for outline fonts (Roboto, Symbols, Math).
//! - **Colour bitmap path** (`stock::text`): swash rasterises emoji
//!   strikes into the size-keyed RGBA `GlyphAtlas`. Each glyph quad is
//!   modulated by white so the bitmap RGB passes through unchanged.
//!
//! Per-glyph routing is decided by the source font's classification
//! (`GlyphAtlas::is_color_font`). Each [`TextRun`] carries a
//! `kind` so the runner knows which pipeline + page descriptor to bind.
//!
//! Atlas dirty uploads run inside a one-shot command buffer that is
//! submitted + waited on inside `flush()`. Batching into the host's
//! main draw command buffer is a future option if profiling demands it.

use std::ops::Range;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use cosmic_text::fontdb;
use damascene_core::ir::TextAnchor;
use damascene_core::shader::stock_wgsl;
use damascene_core::text::atlas::{
    ATLAS_BYTES_PER_PIXEL, AtlasPage, AtlasRect, GlyphAtlas, GlyphSlot, RunStyle, ShapedGlyph,
    ShapedRun,
};
use damascene_core::text::msdf_atlas::{
    DEFAULT_BASE_EM, DEFAULT_SPREAD, MsdfAtlas, MsdfAtlasPage, MsdfGlyphKey, MsdfRect, MsdfSlot,
};
use damascene_core::tree::{Rect, TextWrap};
use smallvec::smallvec;
use ttf_parser::Face;
use vulkano::{
    buffer::{
        Buffer, BufferCreateInfo, BufferUsage, Subbuffer,
        allocator::{SubbufferAllocator, SubbufferAllocatorCreateInfo},
    },
    command_buffer::{
        AutoCommandBufferBuilder, BufferImageCopy, CommandBufferUsage, CopyBufferToImageInfo,
        allocator::StandardCommandBufferAllocator,
    },
    descriptor_set::{
        DescriptorSet, WriteDescriptorSet, allocator::StandardDescriptorSetAllocator,
    },
    device::{Device, Queue},
    format::Format,
    image::{
        Image, ImageAspects, ImageCreateInfo, ImageSubresourceLayers, ImageType, ImageUsage,
        sampler::{Filter, Sampler, SamplerAddressMode, SamplerCreateInfo, SamplerMipmapMode},
        view::ImageView,
    },
    memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator},
    pipeline::{
        DynamicState, GraphicsPipeline, Pipeline, PipelineShaderStageCreateInfo,
        graphics::{
            GraphicsPipelineCreateInfo,
            color_blend::{
                AttachmentBlend, BlendFactor, BlendOp, ColorBlendAttachmentState, ColorBlendState,
            },
            input_assembly::{InputAssemblyState, PrimitiveTopology},
            rasterization::RasterizationState,
            subpass::PipelineSubpassType,
            vertex_input::{
                VertexInputAttributeDescription, VertexInputBindingDescription, VertexInputRate,
                VertexInputState,
            },
            viewport::ViewportState,
        },
    },
    render_pass::Subpass,
    shader::{ShaderModule, ShaderModuleCreateInfo},
    sync::{self, GpuFuture},
};

use damascene_core::color::ColorSpace;
use damascene_core::paint::{DEFAULT_WORKING_COLOR_SPACE, PhysicalScissor, rgba_f32_in};
use damascene_core::runtime::TextRecorder;

use crate::naga_compile::wgsl_to_spirv;
use crate::pipeline::multisample_state;

const INSTANCE_ARENA_SIZE: u64 = 128 * 1024;

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

#[derive(Clone, Copy)]
pub(crate) struct TextRun {
    pub kind: TextRunKind,
    pub page: u32,
    pub scissor: Option<PhysicalScissor>,
    pub first: u32,
    pub count: u32,
}

struct PageGpu {
    image: Arc<Image>,
    descriptor_set: Arc<DescriptorSet>,
}

pub(crate) struct TextPaint {
    pub atlas: GlyphAtlas,
    pub msdf_atlas: MsdfAtlas,

    // Colour bitmap path.
    color_pages: Vec<PageGpu>,
    color_instances: Vec<ColorGlyphInstance>,
    color_instance_alloc: SubbufferAllocator,
    color_instance_buf: Option<Subbuffer<[ColorGlyphInstance]>>,
    color_pipeline: Arc<GraphicsPipeline>,
    color_sampler: Arc<Sampler>,

    // MTSDF outline path.
    msdf_pages: Vec<PageGpu>,
    msdf_instances: Vec<MsdfGlyphInstance>,
    msdf_instance_alloc: SubbufferAllocator,
    msdf_instance_buf: Option<Subbuffer<[MsdfGlyphInstance]>>,
    msdf_pipeline: Arc<GraphicsPipeline>,
    msdf_sampler: Arc<Sampler>,

    // Inline-run highlight path (solid quads behind glyphs).
    highlight_instances: Vec<HighlightInstance>,
    highlight_instance_alloc: SubbufferAllocator,
    highlight_instance_buf: Option<Subbuffer<[HighlightInstance]>>,
    highlight_pipeline: Arc<GraphicsPipeline>,

    runs: Vec<TextRun>,

    memory_alloc: Arc<StandardMemoryAllocator>,
    descriptor_alloc: Arc<StandardDescriptorSetAllocator>,
    cmd_alloc: Arc<StandardCommandBufferAllocator>,
    queue: Arc<Queue>,

    /// Working color space glyph/highlight/decoration colors are
    /// converted into at record time. Kept in sync with the owning
    /// `Runner` via `set_working_color_space`.
    working_color_space: ColorSpace,
}

impl TextPaint {
    pub(crate) fn new(
        device: Arc<Device>,
        queue: Arc<Queue>,
        memory_alloc: Arc<StandardMemoryAllocator>,
        descriptor_alloc: Arc<StandardDescriptorSetAllocator>,
        cmd_alloc: Arc<StandardCommandBufferAllocator>,
        subpass: Subpass,
        sample_count: u32,
    ) -> Self {
        let color_pipeline = build_color_pipeline(device.clone(), subpass.clone(), sample_count);
        let color_sampler = Sampler::new(
            device.clone(),
            SamplerCreateInfo {
                mag_filter: Filter::Linear,
                min_filter: Filter::Linear,
                mipmap_mode: SamplerMipmapMode::Nearest,
                address_mode: [SamplerAddressMode::ClampToEdge; 3],
                ..Default::default()
            },
        )
        .expect("damascene-vulkano: text colour sampler");

        let make_alloc = || {
            SubbufferAllocator::new(
                memory_alloc.clone(),
                SubbufferAllocatorCreateInfo {
                    arena_size: INSTANCE_ARENA_SIZE,
                    buffer_usage: BufferUsage::VERTEX_BUFFER,
                    memory_type_filter: MemoryTypeFilter::PREFER_HOST
                        | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                    ..Default::default()
                },
            )
        };
        let color_instance_alloc = make_alloc();

        let msdf_pipeline = build_msdf_pipeline(device.clone(), subpass.clone(), sample_count);
        let msdf_sampler = Sampler::new(
            device.clone(),
            SamplerCreateInfo {
                mag_filter: Filter::Linear,
                min_filter: Filter::Linear,
                mipmap_mode: SamplerMipmapMode::Nearest,
                address_mode: [SamplerAddressMode::ClampToEdge; 3],
                ..Default::default()
            },
        )
        .expect("damascene-vulkano: text msdf sampler");
        let msdf_instance_alloc = make_alloc();

        let highlight_pipeline = build_highlight_pipeline(device.clone(), subpass, sample_count);
        let highlight_instance_alloc = make_alloc();
        let _ = device;

        Self {
            atlas: GlyphAtlas::new(),
            msdf_atlas: MsdfAtlas::new(DEFAULT_BASE_EM, DEFAULT_SPREAD),
            color_pages: Vec::new(),
            color_instances: Vec::new(),
            color_instance_alloc,
            color_instance_buf: None,
            color_pipeline,
            color_sampler,
            msdf_pages: Vec::new(),
            msdf_instances: Vec::new(),
            msdf_instance_alloc,
            msdf_instance_buf: None,
            msdf_pipeline,
            msdf_sampler,
            highlight_instances: Vec::new(),
            highlight_instance_alloc,
            highlight_instance_buf: None,
            highlight_pipeline,
            runs: Vec::new(),
            memory_alloc,
            descriptor_alloc,
            cmd_alloc,
            queue,
            working_color_space: DEFAULT_WORKING_COLOR_SPACE,
        }
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
        // Shape at the *logical* size: MSDF is unhinted, so glyph IDs
        // and advances scale uniformly with size; we want logical-px
        // positions out so quads land on logical pixels and the SDF
        // shader handles screen-pixel AA via fwidth(uv).
        let avail = wrap_available_width(rect.w, scale_factor, wrap, anchor);
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

        // Layout came back in logical px (we shaped at logical size).
        // Center the whole laid-out block within the rect on NoWrap so
        // multi-line NoWrap text — a code block body, a label that
        // contains an embedded `\n` — stays flush to the top of its
        // hugged rect instead of being shoved down by `(N-1) *
        // line_height / 2`.
        let v_offset = match wrap {
            TextWrap::NoWrap => ((rect.h - shaped.layout.height).max(0.0)) * 0.5,
            TextWrap::Wrap => 0.0,
        };
        let origin_x = rect.x;
        let origin_y = rect.y + v_offset;

        // Inline-run highlights ride at the front of the run sequence
        // so they paint *behind* the glyphs on the same scissor / z
        // band.
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

        // Walk shaped glyphs. Each becomes either a colour or MSDF
        // instance, emitted into its own per-kind run. A run breaks
        // whenever the kind+page combination changes.
        let mut current: Option<(TextRunKind, u32, u32)> = None; // (kind, page, run_first)

        for glyph in &shaped.glyphs {
            let font_id = glyph.key.font;
            let is_color = self.atlas.is_color_font(font_id);
            if is_color {
                self.atlas.ensure_color_glyph(glyph.key);
                let Some(slot) = self.atlas.slot(glyph.key) else {
                    continue;
                };
                if slot.rect.w == 0 || slot.rect.h == 0 {
                    continue;
                }
                let page = slot.page;
                let next_kind = TextRunKind::Color;
                self.maybe_close_run(&mut current, next_kind, page, scissor);
                self.push_color_glyph(glyph, slot, origin_x, origin_y, scale_factor);
            } else {
                let mkey = MsdfGlyphKey {
                    font: font_id,
                    glyph_id: glyph.key.glyph_id,
                };
                let Some(slot) = self.ensure_msdf(mkey, font_id, glyph.key.weight) else {
                    // Whitespace or .notdef without outline — no quad.
                    continue;
                };
                let page = slot.page;
                let next_kind = TextRunKind::Msdf;
                self.maybe_close_run(&mut current, next_kind, page, scissor);
                self.push_msdf_glyph(glyph, slot, origin_x, origin_y);
            }
        }

        // Close the trailing open run.
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

        // Decoration rects (underline / strikethrough). Appended
        // *after* the glyph runs so they paint on top — the existing
        // Highlight pipeline draws solid rgba quads, which is exactly
        // what an underline or strikethrough bar is.
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
        let needs_close = match current {
            Some((kind, page, _)) => *kind != next_kind || *page != next_page,
            None => false,
        };
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
        // Colour-bitmap atlas slots live in physical px (size-keyed
        // GlyphAtlas). Glyph positions came out of shape() in logical
        // px. Divide bitmap pixel metrics by scale_factor so the quad
        // is in logical px while bitmaps still map 1:1 to physical
        // pixels.
        //
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
        let inst_color = if slot.is_color {
            [1.0, 1.0, 1.0, 1.0]
        } else {
            rgba_f32_in(glyph.color, self.working_color_space)
        };
        self.color_instances.push(ColorGlyphInstance {
            rect: [bx, by, bw, bh],
            uv,
            color: inst_color,
        });
    }

    fn push_msdf_glyph(
        &mut self,
        glyph: &ShapedGlyph,
        slot: MsdfSlot,
        origin_x: f32,
        origin_y: f32,
    ) {
        // MSDF slot metrics are in **base-em pixels**; multiply by
        // (logical em / base em) to get logical px.
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
        let color = rgba_f32_in(glyph.color, self.working_color_space);
        self.msdf_instances.push(MsdfGlyphInstance {
            rect: [bx, by, bw, bh],
            uv,
            color,
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
        // get_font requires &mut FontSystem; db().face() requires &.
        // Hop: take Arc<Font> first (drops the mut borrow) so we can
        // re-borrow immutably for the face_index lookup.
        let font = self.atlas.font_system_mut().get_font(font_id, weight)?;
        let face_index = self.atlas.font_system().db().face(font_id)?.index;
        let face = Face::parse(font.data(), face_index).ok()?;
        self.msdf_atlas.ensure(key, &face)
    }

    /// Sync atlas pages to GPU images and upload instance buffers.
    /// Run once per frame after all `record` calls, before the host
    /// records its draw command buffer.
    pub(crate) fn flush(&mut self) {
        // ---- Colour atlas pages ----
        let color_dirty = self.atlas.take_dirty();
        while self.color_pages.len() < self.atlas.pages().len() {
            let i = self.color_pages.len();
            let page = &self.atlas.pages()[i];
            let new_page = self.create_color_page(page.width, page.height);
            self.color_pages.push(new_page);
        }

        // ---- MSDF atlas pages ----
        let msdf_dirty = self.msdf_atlas.take_dirty();
        while self.msdf_pages.len() < self.msdf_atlas.pages().len() {
            let i = self.msdf_pages.len();
            let page = &self.msdf_atlas.pages()[i];
            let new_page = self.create_msdf_page(page.width, page.height);
            self.msdf_pages.push(new_page);
        }

        // ---- Upload all dirty regions in one one-shot command buffer ----
        if !color_dirty.is_empty() || !msdf_dirty.is_empty() {
            let mut builder = AutoCommandBufferBuilder::primary(
                self.cmd_alloc.clone(),
                self.queue.queue_family_index(),
                CommandBufferUsage::OneTimeSubmit,
            )
            .expect("damascene-vulkano: text upload cmd builder");

            for (page_idx, rect) in &color_dirty {
                if rect.w == 0 || rect.h == 0 {
                    continue;
                }
                let page = &self.atlas.pages()[*page_idx];
                let bytes = pack_color_rect_bytes(page, *rect);
                self.append_buffer_to_image_copy(
                    &mut builder,
                    self.color_pages[*page_idx].image.clone(),
                    bytes,
                    [rect.x, rect.y, rect.w, rect.h],
                );
            }
            for (page_idx, rect) in &msdf_dirty {
                if rect.w == 0 || rect.h == 0 {
                    continue;
                }
                let page = &self.msdf_atlas.pages()[*page_idx];
                let bytes = pack_msdf_rect_bytes(page, *rect);
                self.append_buffer_to_image_copy(
                    &mut builder,
                    self.msdf_pages[*page_idx].image.clone(),
                    bytes,
                    [rect.x, rect.y, rect.w, rect.h],
                );
            }

            let cb = builder
                .build()
                .expect("damascene-vulkano: text upload cmd build");
            let future = sync::now(self.queue.device().clone())
                .then_execute(self.queue.clone(), cb)
                .expect("damascene-vulkano: text upload then_execute")
                .then_signal_fence_and_flush()
                .expect("damascene-vulkano: text upload flush");
            future
                .wait(None)
                .expect("damascene-vulkano: text upload fence wait");
        }

        // ---- Per-frame instance suballocations ----
        if self.color_instances.is_empty() {
            self.color_instance_buf = None;
        } else {
            let buf = self
                .color_instance_alloc
                .allocate_slice::<ColorGlyphInstance>(self.color_instances.len() as u64)
                .expect("damascene-vulkano: text colour instance suballocate");
            buf.write()
                .expect("damascene-vulkano: text colour instance suballocation write")
                .copy_from_slice(&self.color_instances);
            self.color_instance_buf = Some(buf);
        }

        if self.msdf_instances.is_empty() {
            self.msdf_instance_buf = None;
        } else {
            let buf = self
                .msdf_instance_alloc
                .allocate_slice::<MsdfGlyphInstance>(self.msdf_instances.len() as u64)
                .expect("damascene-vulkano: text msdf instance suballocate");
            buf.write()
                .expect("damascene-vulkano: text msdf instance suballocation write")
                .copy_from_slice(&self.msdf_instances);
            self.msdf_instance_buf = Some(buf);
        }

        if self.highlight_instances.is_empty() {
            self.highlight_instance_buf = None;
        } else {
            let buf = self
                .highlight_instance_alloc
                .allocate_slice::<HighlightInstance>(self.highlight_instances.len() as u64)
                .expect("damascene-vulkano: text highlight instance suballocate");
            buf.write()
                .expect("damascene-vulkano: text highlight instance suballocation write")
                .copy_from_slice(&self.highlight_instances);
            self.highlight_instance_buf = Some(buf);
        }
    }

    fn append_buffer_to_image_copy(
        &self,
        builder: &mut AutoCommandBufferBuilder<vulkano::command_buffer::PrimaryAutoCommandBuffer>,
        target: Arc<Image>,
        bytes: Vec<u8>,
        rect: [u32; 4],
    ) {
        let staging = Buffer::from_iter(
            self.memory_alloc.clone(),
            BufferCreateInfo {
                usage: BufferUsage::TRANSFER_SRC,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            bytes,
        )
        .expect("damascene-vulkano: text staging buffer");
        let copy_info = CopyBufferToImageInfo {
            regions: smallvec![BufferImageCopy {
                buffer_offset: 0,
                buffer_row_length: 0,
                buffer_image_height: 0,
                image_subresource: ImageSubresourceLayers {
                    aspects: ImageAspects::COLOR,
                    mip_level: 0,
                    array_layers: 0..1,
                },
                image_offset: [rect[0], rect[1], 0],
                image_extent: [rect[2], rect[3], 1],
                ..Default::default()
            }],
            ..CopyBufferToImageInfo::buffer_image(staging, target)
        };
        builder
            .copy_buffer_to_image(copy_info)
            .expect("damascene-vulkano: text copy_buffer_to_image");
    }

    fn create_color_page(&self, width: u32, height: u32) -> PageGpu {
        let image = Image::new(
            self.memory_alloc.clone(),
            ImageCreateInfo {
                image_type: ImageType::Dim2d,
                format: Format::R8G8B8A8_SRGB,
                extent: [width, height, 1],
                usage: ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
        )
        .expect("damascene-vulkano: text colour atlas page image");
        let view = ImageView::new_default(image.clone())
            .expect("damascene-vulkano: text colour page view");
        let descriptor_set = DescriptorSet::new(
            self.descriptor_alloc.clone(),
            self.color_pipeline.layout().set_layouts()[1].clone(),
            [
                WriteDescriptorSet::image_view(0, view),
                WriteDescriptorSet::sampler(1, self.color_sampler.clone()),
            ],
            [],
        )
        .expect("damascene-vulkano: text colour page descriptor set");
        PageGpu {
            image,
            descriptor_set,
        }
    }

    fn create_msdf_page(&self, width: u32, height: u32) -> PageGpu {
        let image = Image::new(
            self.memory_alloc.clone(),
            ImageCreateInfo {
                image_type: ImageType::Dim2d,
                // Linear (NOT sRGB) — distance bytes shouldn't pass
                // through the sRGB EOTF.
                format: Format::R8G8B8A8_UNORM,
                extent: [width, height, 1],
                usage: ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
        )
        .expect("damascene-vulkano: text msdf atlas page image");
        let view =
            ImageView::new_default(image.clone()).expect("damascene-vulkano: text msdf page view");
        let descriptor_set = DescriptorSet::new(
            self.descriptor_alloc.clone(),
            self.msdf_pipeline.layout().set_layouts()[1].clone(),
            [
                WriteDescriptorSet::image_view(0, view),
                WriteDescriptorSet::sampler(1, self.msdf_sampler.clone()),
            ],
            [],
        )
        .expect("damascene-vulkano: text msdf page descriptor set");
        PageGpu {
            image,
            descriptor_set,
        }
    }

    pub(crate) fn run(&self, index: usize) -> TextRun {
        self.runs[index]
    }

    pub(crate) fn pipeline_for(&self, kind: TextRunKind) -> &Arc<GraphicsPipeline> {
        match kind {
            TextRunKind::Color => &self.color_pipeline,
            TextRunKind::Msdf => &self.msdf_pipeline,
            TextRunKind::Highlight => &self.highlight_pipeline,
        }
    }

    /// Page descriptor set for textured glyph kinds. `Highlight` runs
    /// have no page binding and must be filtered out before calling.
    pub(crate) fn page_descriptor(&self, kind: TextRunKind, page: u32) -> &Arc<DescriptorSet> {
        match kind {
            TextRunKind::Color => &self.color_pages[page as usize].descriptor_set,
            TextRunKind::Msdf => &self.msdf_pages[page as usize].descriptor_set,
            TextRunKind::Highlight => unreachable!("highlight runs carry no page binding"),
        }
    }

    /// Per-frame colour-glyph instance suballocation. Bind sites are
    /// gated by the `TextRunKind::Color` arm, which `record(...)` only
    /// emits when `color_instances` is non-empty.
    pub(crate) fn instance_buf_color(&self) -> &Subbuffer<[ColorGlyphInstance]> {
        self.color_instance_buf
            .as_ref()
            .expect("damascene-vulkano: text instance_buf_color accessed with no draws")
    }

    pub(crate) fn instance_buf_msdf(&self) -> &Subbuffer<[MsdfGlyphInstance]> {
        self.msdf_instance_buf
            .as_ref()
            .expect("damascene-vulkano: text instance_buf_msdf accessed with no draws")
    }

    pub(crate) fn instance_buf_highlight(&self) -> &Subbuffer<[HighlightInstance]> {
        self.highlight_instance_buf
            .as_ref()
            .expect("damascene-vulkano: text instance_buf_highlight accessed with no draws")
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

fn wrap_available_width(
    rect_w: f32,
    _scale_factor: f32,
    wrap: TextWrap,
    anchor: TextAnchor,
) -> Option<f32> {
    // We shape at logical px now, so the available width is logical
    // too — no scale_factor multiplication.
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

fn build_color_pipeline(
    device: Arc<Device>,
    subpass: Subpass,
    sample_count: u32,
) -> Arc<GraphicsPipeline> {
    let words = wgsl_to_spirv("stock::text", stock_wgsl::TEXT)
        .unwrap_or_else(|e| panic!("damascene-vulkano: text WGSL compile: {e}"));
    let module = unsafe {
        ShaderModule::new(device.clone(), ShaderModuleCreateInfo::new(&words))
            .expect("damascene-vulkano: text ShaderModule::new")
    };
    let vs = module
        .entry_point("vs_main")
        .expect("text.wgsl: missing vs_main");
    let fs = module
        .entry_point("fs_main")
        .expect("text.wgsl: missing fs_main");
    let stages = [
        PipelineShaderStageCreateInfo::new(vs),
        PipelineShaderStageCreateInfo::new(fs),
    ];
    let layout = crate::pipeline::build_shared_pipeline_layout(device.clone(), &stages);

    let bind_vertex = VertexInputBindingDescription {
        stride: (2 * std::mem::size_of::<f32>()) as u32,
        input_rate: VertexInputRate::Vertex,
        ..Default::default()
    };
    let bind_instance = VertexInputBindingDescription {
        stride: std::mem::size_of::<ColorGlyphInstance>() as u32,
        input_rate: VertexInputRate::Instance { divisor: 1 },
        ..Default::default()
    };
    let attr = |binding: u32, offset: u32, format: Format| VertexInputAttributeDescription {
        binding,
        offset,
        format,
        ..Default::default()
    };
    let vertex_input_state = VertexInputState::new()
        .binding(0, bind_vertex)
        .binding(1, bind_instance)
        .attribute(0, attr(0, 0, Format::R32G32_SFLOAT))
        .attribute(1, attr(1, 0, Format::R32G32B32A32_SFLOAT))
        .attribute(2, attr(1, 16, Format::R32G32B32A32_SFLOAT))
        .attribute(3, attr(1, 32, Format::R32G32B32A32_SFLOAT));

    let premultiplied = AttachmentBlend {
        src_color_blend_factor: BlendFactor::One,
        dst_color_blend_factor: BlendFactor::OneMinusSrcAlpha,
        color_blend_op: BlendOp::Add,
        src_alpha_blend_factor: BlendFactor::One,
        dst_alpha_blend_factor: BlendFactor::OneMinusSrcAlpha,
        alpha_blend_op: BlendOp::Add,
    };

    GraphicsPipeline::new(
        device,
        None,
        GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(vertex_input_state),
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
                    blend: Some(premultiplied),
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
    .expect("damascene-vulkano: text colour GraphicsPipeline::new")
}

fn build_msdf_pipeline(
    device: Arc<Device>,
    subpass: Subpass,
    sample_count: u32,
) -> Arc<GraphicsPipeline> {
    let words = wgsl_to_spirv("stock::text_msdf", stock_wgsl::TEXT_MSDF)
        .unwrap_or_else(|e| panic!("damascene-vulkano: text msdf WGSL compile: {e}"));
    let module = unsafe {
        ShaderModule::new(device.clone(), ShaderModuleCreateInfo::new(&words))
            .expect("damascene-vulkano: text msdf ShaderModule::new")
    };
    let vs = module
        .entry_point("vs_main")
        .expect("text_msdf.wgsl: missing vs_main");
    let fs = module
        .entry_point("fs_main")
        .expect("text_msdf.wgsl: missing fs_main");
    let stages = [
        PipelineShaderStageCreateInfo::new(vs),
        PipelineShaderStageCreateInfo::new(fs),
    ];
    let layout = crate::pipeline::build_shared_pipeline_layout(device.clone(), &stages);

    let bind_vertex = VertexInputBindingDescription {
        stride: (2 * std::mem::size_of::<f32>()) as u32,
        input_rate: VertexInputRate::Vertex,
        ..Default::default()
    };
    let bind_instance = VertexInputBindingDescription {
        stride: std::mem::size_of::<MsdfGlyphInstance>() as u32,
        input_rate: VertexInputRate::Instance { divisor: 1 },
        ..Default::default()
    };
    let attr = |binding: u32, offset: u32, format: Format| VertexInputAttributeDescription {
        binding,
        offset,
        format,
        ..Default::default()
    };
    let vertex_input_state = VertexInputState::new()
        .binding(0, bind_vertex)
        .binding(1, bind_instance)
        .attribute(0, attr(0, 0, Format::R32G32_SFLOAT))
        .attribute(1, attr(1, 0, Format::R32G32B32A32_SFLOAT))
        .attribute(2, attr(1, 16, Format::R32G32B32A32_SFLOAT))
        .attribute(3, attr(1, 32, Format::R32G32B32A32_SFLOAT))
        .attribute(4, attr(1, 48, Format::R32G32B32A32_SFLOAT));

    let premultiplied = AttachmentBlend {
        src_color_blend_factor: BlendFactor::One,
        dst_color_blend_factor: BlendFactor::OneMinusSrcAlpha,
        color_blend_op: BlendOp::Add,
        src_alpha_blend_factor: BlendFactor::One,
        dst_alpha_blend_factor: BlendFactor::OneMinusSrcAlpha,
        alpha_blend_op: BlendOp::Add,
    };

    GraphicsPipeline::new(
        device,
        None,
        GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(vertex_input_state),
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
                    blend: Some(premultiplied),
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
    .expect("damascene-vulkano: text msdf GraphicsPipeline::new")
}

fn build_highlight_pipeline(
    device: Arc<Device>,
    subpass: Subpass,
    sample_count: u32,
) -> Arc<GraphicsPipeline> {
    let words = wgsl_to_spirv("stock::text_highlight", stock_wgsl::TEXT_HIGHLIGHT)
        .unwrap_or_else(|e| panic!("damascene-vulkano: text highlight WGSL compile: {e}"));
    let module = unsafe {
        ShaderModule::new(device.clone(), ShaderModuleCreateInfo::new(&words))
            .expect("damascene-vulkano: text highlight ShaderModule::new")
    };
    let vs = module
        .entry_point("vs_main")
        .expect("text_highlight.wgsl: missing vs_main");
    let fs = module
        .entry_point("fs_main")
        .expect("text_highlight.wgsl: missing fs_main");
    let stages = [
        PipelineShaderStageCreateInfo::new(vs),
        PipelineShaderStageCreateInfo::new(fs),
    ];
    let layout = crate::pipeline::build_shared_pipeline_layout(device.clone(), &stages);

    let bind_vertex = VertexInputBindingDescription {
        stride: (2 * std::mem::size_of::<f32>()) as u32,
        input_rate: VertexInputRate::Vertex,
        ..Default::default()
    };
    let bind_instance = VertexInputBindingDescription {
        stride: std::mem::size_of::<HighlightInstance>() as u32,
        input_rate: VertexInputRate::Instance { divisor: 1 },
        ..Default::default()
    };
    let attr = |binding: u32, offset: u32, format: Format| VertexInputAttributeDescription {
        binding,
        offset,
        format,
        ..Default::default()
    };
    let vertex_input_state = VertexInputState::new()
        .binding(0, bind_vertex)
        .binding(1, bind_instance)
        .attribute(0, attr(0, 0, Format::R32G32_SFLOAT))
        .attribute(1, attr(1, 0, Format::R32G32B32A32_SFLOAT))
        .attribute(2, attr(1, 16, Format::R32G32B32A32_SFLOAT));

    let premultiplied = AttachmentBlend {
        src_color_blend_factor: BlendFactor::One,
        dst_color_blend_factor: BlendFactor::OneMinusSrcAlpha,
        color_blend_op: BlendOp::Add,
        src_alpha_blend_factor: BlendFactor::One,
        dst_alpha_blend_factor: BlendFactor::OneMinusSrcAlpha,
        alpha_blend_op: BlendOp::Add,
    };

    GraphicsPipeline::new(
        device,
        None,
        GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(vertex_input_state),
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
                    blend: Some(premultiplied),
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
    .expect("damascene-vulkano: text highlight GraphicsPipeline::new")
}
