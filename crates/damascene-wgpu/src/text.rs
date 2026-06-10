//! Text rendering: MSDF for outline glyphs, RGBA bitmap for colour
//! glyphs.
//!
//! Both paths share one [`damascene_core::text::atlas::GlyphAtlas`] for
//! shaping (cosmic-text + rustybuzz). After shaping, the recorder walks
//! the [`ShapedRun`] and routes each glyph by source-font kind:
//!
//! - **Outline fonts** (Roboto, Inter, Symbols, Math) — rasterized once
//!   per `(font, glyph)` into the [`MsdfAtlas`] and rendered through
//!   `stock::text_msdf` with screen-space-derivative AA. The atlas is
//!   size-independent: a single MSDF serves every UI size and every
//!   display scale.
//!
//! - **Colour fonts** (NotoColorEmoji, COLR Material Symbols) — swash
//!   rasterizes the strike that best matches the requested size into
//!   the legacy RGBA atlas, rendered through `stock::text` (modulate by
//!   white = passthrough).
//!
//! Each [`TextRun`] is one of [`TextRunKind::Msdf`] / [`TextRunKind::Color`];
//! the renderer reads `kind` to choose pipeline + page bind group.

use std::borrow::Cow;

use damascene_core::ir::TextAnchor;
use damascene_core::shader::stock_wgsl;
use damascene_core::text::atlas::{
    ATLAS_BYTES_PER_PIXEL, AtlasPage, AtlasRect, GlyphAtlas, RunStyle, ShapedGlyph, ShapedRun,
};
use damascene_core::text::msdf_atlas::{
    DEFAULT_BASE_EM, DEFAULT_SPREAD, MSDF_BYTES_PER_PIXEL, MsdfAtlas, MsdfAtlasPage, MsdfGlyphKey,
    MsdfRect, MsdfSlot,
};
use damascene_core::tree::{FontFamily, Rect, TextWrap};

use bytemuck::{Pod, Zeroable};
use cosmic_text::fontdb;
use ttf_parser::Face;

use damascene_core::color::ColorSpace;
use damascene_core::paint::{DEFAULT_WORKING_COLOR_SPACE, PhysicalScissor, rgba_f32_in};
use damascene_core::runtime::TextRecorder;

const INITIAL_INSTANCE_CAPACITY: usize = 256;

const COLOR_INSTANCE_ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
    1 => Float32x4,  // rect  (xy = top-left logical px, zw = size logical px)
    2 => Float32x4,  // uv    (xy = uv 0..1, zw = uv size 0..1)
    3 => Float32x4,  // color (linear rgba 0..1)
];

const MSDF_INSTANCE_ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    1 => Float32x4,  // rect
    2 => Float32x4,  // uv
    3 => Float32x4,  // color
    4 => Float32x4,  // params (x = atlas-space spread, y/z/w reserved)
];

const HIGHLIGHT_INSTANCE_ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
    1 => Float32x4,  // rect  (xy = top-left logical px, zw = size logical px)
    2 => Float32x4,  // color (linear rgba 0..1)
];

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

#[derive(Clone, Copy, PartialEq, Eq)]
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

struct PageTexture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

/// The device-scoped half of text rendering, shareable across
/// [`Runner`](crate::Runner)s (issue #94): the font system + shaping
/// cache, the CPU-side glyph and MSDF atlases, and their GPU page
/// textures with bind groups. Everything here is independent of the
/// swapchain format and MSAA sample count, so one `SharedText` per
/// `wgpu::Device` can back every window — a multi-window host that
/// passes the same handle to each `Runner`
/// ([`Runner::with_shared_text`](crate::Runner::with_shared_text))
/// pays glyph rasterization, shaping, warm-up, and atlas VRAM once per
/// device instead of once per window.
///
/// Cloning is cheap (an `Arc`); the inner state is mutex-guarded and
/// locked per record/flush call, so windows can be prepared from one
/// thread in any order. Each attached `Runner` widens the atlases'
/// LRU protection window (see
/// `MsdfAtlas::set_lru_protection_window`) so a page referenced by one
/// window's in-flight frame can't be recycled by another's prepare.
///
/// The default `Runner` constructors create a private `SharedText`
/// per runner — single-window behavior is unchanged.
#[derive(Clone)]
pub struct SharedText(pub(crate) std::sync::Arc<std::sync::Mutex<SharedTextInner>>);

pub(crate) struct SharedTextInner {
    pub(crate) atlas: GlyphAtlas,
    pub(crate) msdf_atlas: MsdfAtlas,

    color_pages: Vec<PageTexture>,
    color_page_bind_layout: wgpu::BindGroupLayout,
    color_sampler: wgpu::Sampler,

    msdf_pages: Vec<PageTexture>,
    msdf_page_bind_layout: wgpu::BindGroupLayout,
    msdf_sampler: wgpu::Sampler,

    /// Number of `TextPaint`s currently attached — mirrored into both
    /// atlases' LRU protection windows so recycling stays safe under
    /// any prepare/render interleaving across the attached runners.
    attached: u32,
}

impl SharedText {
    /// A fresh shared text pool for `device`. Pass the same handle to
    /// every [`Runner`](crate::Runner) created on that device. Do
    /// **not** share one `SharedText` across devices — the page
    /// textures belong to the device that created them.
    pub fn new(device: &wgpu::Device) -> Self {
        let color_page_bind_layout = create_page_bind_layout(device, "color");
        let msdf_page_bind_layout = create_page_bind_layout(device, "msdf");
        let color_sampler = create_page_sampler(device, "color");
        let msdf_sampler = create_page_sampler(device, "msdf");
        Self(std::sync::Arc::new(std::sync::Mutex::new(
            SharedTextInner {
                atlas: GlyphAtlas::new(),
                msdf_atlas: MsdfAtlas::new(DEFAULT_BASE_EM, DEFAULT_SPREAD),
                color_pages: Vec::new(),
                color_page_bind_layout,
                color_sampler,
                msdf_pages: Vec::new(),
                msdf_page_bind_layout,
                msdf_sampler,
                attached: 0,
            },
        )))
    }

    /// Pre-rasterize printable ASCII for the bundled default faces —
    /// see [`TextPaint::warm_default_glyphs`] for cost and rationale.
    /// On a shared pool this runs once per *device*: warm the pool
    /// before (or after) attaching runners, and every attached runner
    /// is warm. Runners attached to an already-warm pool skip the cost
    /// in their own `warm_default_glyphs` automatically (rasterized
    /// glyphs are cache hits).
    pub fn warm_default_glyphs(&self) {
        self.lock().warm_default_glyphs();
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, SharedTextInner> {
        // Glyph rasterization can't poison anything we can't keep
        // using; recover the guard rather than propagating panics
        // across windows.
        match self.0.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

pub(crate) struct TextPaint {
    /// Device-scoped shared half: atlases, page textures, bind groups.
    shared: SharedText,

    // Per-window bind-group snapshots, cloned from the shared pool at
    // `flush` so `render` never takes the lock (and a page texture
    // created by another window's later flush can't shift indices
    // under this window's recorded runs — wgpu resources are
    // internally ref-counted, so clones are cheap handles).
    color_page_bgs: Vec<wgpu::BindGroup>,
    msdf_page_bgs: Vec<wgpu::BindGroup>,

    // Colour-bitmap path (NotoColorEmoji, COLR fonts).
    color_instances: Vec<ColorGlyphInstance>,
    color_instance_buf: wgpu::Buffer,
    color_instance_capacity: usize,
    color_pipeline: wgpu::RenderPipeline,

    // MSDF outline path.
    msdf_instances: Vec<MsdfGlyphInstance>,
    msdf_instance_buf: wgpu::Buffer,
    msdf_instance_capacity: usize,
    msdf_pipeline: wgpu::RenderPipeline,

    // Inline-run highlight path (solid quads behind glyphs).
    highlight_instances: Vec<HighlightInstance>,
    highlight_instance_buf: wgpu::Buffer,
    highlight_instance_capacity: usize,
    highlight_pipeline: wgpu::RenderPipeline,

    // Pipeline layouts + sample count retained so the three
    // swapchain-format-bound pipelines above can be rebuilt in place when
    // the host renegotiates the surface format (`set_target_format`). The
    // layouts reference the shared pool's page bind-group layouts, which
    // outlive the pipelines they feed.
    color_pipeline_layout: wgpu::PipelineLayout,
    msdf_pipeline_layout: wgpu::PipelineLayout,
    highlight_pipeline_layout: wgpu::PipelineLayout,
    sample_count: u32,

    runs: Vec<TextRun>,

    /// Working color space glyph + highlight colors are converted into.
    /// Kept in sync with [`RunnerCore::working_color_space`](damascene_core::runtime::RunnerCore::working_color_space)
    /// by the owning `Runner`. Per-window: two windows sharing a pool
    /// can composite in different spaces.
    working_color_space: ColorSpace,
}

impl Drop for TextPaint {
    fn drop(&mut self) {
        let mut inner = self.shared.lock();
        inner.attached = inner.attached.saturating_sub(1);
        let n = inner.attached.max(1);
        inner.atlas.set_lru_protection_window(n);
        inner.msdf_atlas.set_lru_protection_window(n);
    }
}

/// Page bind-group layout for either glyph-page kind — one filterable
/// 2D texture + one filtering sampler.
fn create_page_bind_layout(device: &wgpu::Device, kind: &str) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(&format!("damascene_wgpu::text::{kind}_page_bind_layout")),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

fn create_page_sampler(device: &wgpu::Device, kind: &str) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(&format!("damascene_wgpu::text::{kind}_sampler")),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    })
}

impl TextPaint {
    pub(crate) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        sample_count: u32,
        frame_bind_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        Self::with_shared(
            device,
            target_format,
            sample_count,
            frame_bind_layout,
            SharedText::new(device),
        )
    }

    /// Build the per-window half against an existing shared pool. The
    /// pool's page bind-group layouts feed this window's pipeline
    /// layouts, so the shared page bind groups bind directly into the
    /// window's pipelines.
    pub(crate) fn with_shared(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        sample_count: u32,
        frame_bind_layout: &wgpu::BindGroupLayout,
        shared: SharedText,
    ) -> Self {
        let (color_pipeline_layout, msdf_pipeline_layout) = {
            let mut inner = shared.lock();
            inner.attached += 1;
            let n = inner.attached;
            inner.atlas.set_lru_protection_window(n);
            inner.msdf_atlas.set_lru_protection_window(n);
            (
                device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("damascene_wgpu::text::color_pipeline_layout"),
                    bind_group_layouts: &[
                        Some(frame_bind_layout),
                        Some(&inner.color_page_bind_layout),
                    ],
                    immediate_size: 0,
                }),
                device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("damascene_wgpu::text::msdf_pipeline_layout"),
                    bind_group_layouts: &[
                        Some(frame_bind_layout),
                        Some(&inner.msdf_page_bind_layout),
                    ],
                    immediate_size: 0,
                }),
            )
        };

        let color_pipeline =
            build_color_pipeline(device, &color_pipeline_layout, target_format, sample_count);
        let msdf_pipeline =
            build_msdf_pipeline(device, &msdf_pipeline_layout, target_format, sample_count);

        let color_instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("damascene_wgpu::text::color_instance_buf"),
            size: (INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<ColorGlyphInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let msdf_instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("damascene_wgpu::text::msdf_instance_buf"),
            size: (INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<MsdfGlyphInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // ---- Inline-run highlight pipeline (`stock::text_highlight`) ----
        // Solid colour quads only — no page texture, just frame uniforms.
        let highlight_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("damascene_wgpu::text::highlight_pipeline_layout"),
                bind_group_layouts: &[Some(frame_bind_layout)],
                immediate_size: 0,
            });
        let highlight_pipeline = build_highlight_pipeline(
            device,
            &highlight_pipeline_layout,
            target_format,
            sample_count,
        );
        let highlight_instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("damascene_wgpu::text::highlight_instance_buf"),
            size: (INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<HighlightInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            shared,
            color_page_bgs: Vec::new(),
            msdf_page_bgs: Vec::new(),
            color_instances: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY),
            color_instance_buf,
            color_instance_capacity: INITIAL_INSTANCE_CAPACITY,
            color_pipeline,
            msdf_instances: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY),
            msdf_instance_buf,
            msdf_instance_capacity: INITIAL_INSTANCE_CAPACITY,
            msdf_pipeline,
            highlight_instances: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY),
            highlight_instance_buf,
            highlight_instance_capacity: INITIAL_INSTANCE_CAPACITY,
            highlight_pipeline,
            color_pipeline_layout,
            msdf_pipeline_layout,
            highlight_pipeline_layout,
            sample_count,
            runs: Vec::new(),
            working_color_space: DEFAULT_WORKING_COLOR_SPACE,
        }
    }

    /// The shared pool this paint records into — for `Runner` to hand
    /// out so further runners can attach to it.
    pub(crate) fn shared(&self) -> &SharedText {
        &self.shared
    }

    /// Update the working color space subsequent glyph / highlight color
    /// packing converts into. Called by `Runner::set_working_color_space`.
    pub(crate) fn set_working_color_space(&mut self, space: ColorSpace) {
        self.working_color_space = space;
    }

    /// Rebuild the three swapchain-format-bound pipelines for a new target
    /// format, preserving atlases, page textures, instance buffers, and
    /// samplers. Called by `Runner::set_target_format` on live surface-format
    /// renegotiation (e.g. SDR ↔ HDR). The pipeline layouts and page
    /// bind-group layouts are unchanged, so the cached page bind groups stay
    /// valid — only the pipelines, which carry the `ColorTargetState.format`,
    /// are recreated.
    pub(crate) fn set_target_format(
        &mut self,
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
    ) {
        self.color_pipeline = build_color_pipeline(
            device,
            &self.color_pipeline_layout,
            target_format,
            self.sample_count,
        );
        self.msdf_pipeline = build_msdf_pipeline(
            device,
            &self.msdf_pipeline_layout,
            target_format,
            self.sample_count,
        );
        self.highlight_pipeline = build_highlight_pipeline(
            device,
            &self.highlight_pipeline_layout,
            target_format,
            self.sample_count,
        );
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
    ) -> std::ops::Range<usize> {
        // Shape at the *logical* size: MSDF is unhinted so size doesn't
        // affect glyph IDs/advances beyond a uniform scale; we want
        // logical-px positions out so quads land on logical pixels and
        // the SDF shader handles screen-pixel AA via fwidth.
        let avail = wrap_available_width(rect.w, scale_factor, wrap, anchor);
        let runs_ref: Vec<(&str, RunStyle)> = runs
            .iter()
            .map(|(text, style)| (text.as_str(), style.clone()))
            .collect();
        // One lock per recorded text op: shaping and atlas slot
        // lookups both touch the shared pool. Uncontended in the
        // single-window case; in a multi-window host windows prepare
        // sequentially on the event-loop thread, so contention stays
        // momentary.
        let shared = self.shared.clone();
        let mut inner = shared.lock();
        let shaped = {
            damascene_core::profile_span!("paint::text::shape_runs");
            inner.atlas.shape_runs_with_line_height(
                &runs_ref,
                size,
                line_height,
                wrap,
                anchor,
                avail,
            )
        };
        damascene_core::profile_span!("paint::text::emit_shaped");
        self.emit_shaped_glyphs(&mut inner, rect, scissor, &shaped, wrap, scale_factor)
    }

    fn emit_shaped_glyphs(
        &mut self,
        inner: &mut SharedTextInner,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        shaped: &ShapedRun,
        wrap: TextWrap,
        scale_factor: f32,
    ) -> std::ops::Range<usize> {
        let runs_start = self.runs.len();
        if shaped.glyphs.is_empty() && shaped.highlights.is_empty() && shaped.decorations.is_empty()
        {
            return runs_start..runs_start;
        }

        // Layout came back in logical px (we shaped at logical size).
        // For NoWrap text we vertically center the whole laid-out
        // block — buttons / badges hand us a control-height rect with
        // a single-line label, and centering reads as "right". Using
        // `layout.height` (rather than one line-height) keeps
        // multi-line NoWrap text — a code block body, a label with an
        // embedded `\n` — flush to the top of its hugged rect instead
        // of being pushed down by `(N-1) * line_height / 2`.
        let v_offset = match wrap {
            TextWrap::NoWrap => ((rect.h - shaped.layout.height).max(0.0)) * 0.5,
            TextWrap::Wrap => 0.0,
        };
        let origin_x = rect.x;
        let origin_y = rect.y + v_offset;

        // Inline-run highlights ride at the front of the run sequence
        // so they paint *behind* the glyphs on the same scissor / z
        // band. Each shaped highlight already represents one line's
        // span of one styled run; we emit them all into a single
        // Highlight TextRun.
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
            let is_color = inner.atlas.is_color_font(font_id);
            if is_color {
                inner.atlas.ensure_color_glyph(glyph.key);
                let Some(slot) = inner.atlas.slot(glyph.key) else {
                    continue;
                };
                if slot.rect.w == 0 || slot.rect.h == 0 {
                    continue;
                }
                let page = slot.page;
                let next_kind = TextRunKind::Color;
                self.maybe_close_run(&mut current, next_kind, page, scissor);
                self.push_color_glyph(inner, glyph, slot, origin_x, origin_y, scale_factor);
            } else {
                let mkey = MsdfGlyphKey {
                    font: font_id,
                    glyph_id: glyph.key.glyph_id,
                };
                let Some(slot) = ensure_msdf(inner, mkey, font_id, glyph.key.weight) else {
                    // Whitespace or .notdef without outline — no quad,
                    // advance is already baked into cosmic-text positions.
                    continue;
                };
                let page = slot.page;
                let next_kind = TextRunKind::Msdf;
                self.maybe_close_run(&mut current, next_kind, page, scissor);
                self.push_msdf_glyph(inner, glyph, slot, origin_x, origin_y);
            }
        }

        // Close the trailing open run, if any.
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
            Some((kind, page, _)) => !same_kind(*kind, next_kind) || *page != next_page,
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
        inner: &SharedTextInner,
        glyph: &ShapedGlyph,
        slot: damascene_core::text::atlas::GlyphSlot,
        origin_x: f32,
        origin_y: f32,
        scale_factor: f32,
    ) {
        // Colour-bitmap atlas slots are in physical px (the atlas is
        // size-keyed). The glyph positions came out of shape() in
        // *logical* px (we shape at logical size). We still want the
        // bitmap rendered crisp per physical pixel — the slot's pixel
        // bounds map 1:1 to physical pixels — so divide bitmap pixel
        // metrics by scale_factor to produce a logical-px quad.
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
        let atlas_page = inner
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
        inner: &SharedTextInner,
        glyph: &ShapedGlyph,
        slot: MsdfSlot,
        origin_x: f32,
        origin_y: f32,
    ) {
        // MSDF slot metrics are in **base-em pixels**. Multiply by the
        // ratio of logical-em / base-em to get logical px.
        let logical_em = glyph.key.size();
        let base_em = inner.msdf_atlas.base_em() as f32;
        let scale = logical_em / base_em;
        let bx = origin_x + glyph.x + slot.bearing_x * scale;
        let by = origin_y + glyph.y + slot.bearing_y * scale;
        let bw = slot.rect.w as f32 * scale;
        let bh = slot.rect.h as f32 * scale;
        let atlas_page = inner
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

    /// Pre-rasterize printable ASCII (0x20–0x7E) for the bundled
    /// proportional and monospace default faces (Inter Variable +
    /// JetBrains Mono Variable). Call once at host startup to absorb
    /// the per-glyph SDF generation cost up-front instead of having
    /// the first frame that introduces each character pay it as a
    /// 20-30ms paint hitch. Glyphs in MSDF are size-independent
    /// (`MsdfGlyphKey { font, glyph_id }` carries no size), and the
    /// bundled faces are variable, so each character is rasterized
    /// exactly once across all weights and sizes. Roughly ~190
    /// rasterizations × ~200µs each ≈ 40ms one-time cost. On a shared
    /// pool ([`SharedText`]) the cost is per *device*: a second runner
    /// attached to a warm pool finds every glyph already cached.
    pub fn warm_default_glyphs(&mut self) {
        self.shared.clone().lock().warm_default_glyphs();
    }

    /// Sync atlas pages to GPU textures, snapshot their bind groups,
    /// and upload instance data.
    pub(crate) fn flush(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        {
            let shared = self.shared.clone();
            let mut inner = shared.lock();
            inner.flush_pages(device, queue);
            // Snapshot the page bind groups this window's recorded runs
            // reference. Clones are cheap Arc bumps; holding them here
            // keeps `render` lock-free and pins the textures for the
            // frame even if the shared pool grows afterwards.
            self.color_page_bgs = inner
                .color_pages
                .iter()
                .map(|p| p.bind_group.clone())
                .collect();
            self.msdf_page_bgs = inner
                .msdf_pages
                .iter()
                .map(|p| p.bind_group.clone())
                .collect();
        }

        // Colour instance buffer.
        if self.color_instances.len() > self.color_instance_capacity {
            let new_cap = self.color_instances.len().next_power_of_two();
            self.color_instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("damascene_wgpu::text::color_instance_buf (resized)"),
                size: (new_cap * std::mem::size_of::<ColorGlyphInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.color_instance_capacity = new_cap;
        }
        if !self.color_instances.is_empty() {
            queue.write_buffer(
                &self.color_instance_buf,
                0,
                bytemuck::cast_slice(&self.color_instances),
            );
        }

        // MSDF instance buffer.
        if self.msdf_instances.len() > self.msdf_instance_capacity {
            let new_cap = self.msdf_instances.len().next_power_of_two();
            self.msdf_instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("damascene_wgpu::text::msdf_instance_buf (resized)"),
                size: (new_cap * std::mem::size_of::<MsdfGlyphInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.msdf_instance_capacity = new_cap;
        }
        if !self.msdf_instances.is_empty() {
            queue.write_buffer(
                &self.msdf_instance_buf,
                0,
                bytemuck::cast_slice(&self.msdf_instances),
            );
        }

        // Highlight instance buffer.
        if self.highlight_instances.len() > self.highlight_instance_capacity {
            let new_cap = self.highlight_instances.len().next_power_of_two();
            self.highlight_instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("damascene_wgpu::text::highlight_instance_buf (resized)"),
                size: (new_cap * std::mem::size_of::<HighlightInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.highlight_instance_capacity = new_cap;
        }
        if !self.highlight_instances.is_empty() {
            queue.write_buffer(
                &self.highlight_instance_buf,
                0,
                bytemuck::cast_slice(&self.highlight_instances),
            );
        }
    }

    pub(crate) fn run(&self, index: usize) -> TextRun {
        self.runs[index]
    }

    pub(crate) fn pipeline_for(&self, kind: TextRunKind) -> &wgpu::RenderPipeline {
        match kind {
            TextRunKind::Color => &self.color_pipeline,
            TextRunKind::Msdf => &self.msdf_pipeline,
            TextRunKind::Highlight => &self.highlight_pipeline,
        }
    }

    pub(crate) fn instance_buf_for(&self, kind: TextRunKind) -> &wgpu::Buffer {
        match kind {
            TextRunKind::Color => &self.color_instance_buf,
            TextRunKind::Msdf => &self.msdf_instance_buf,
            TextRunKind::Highlight => &self.highlight_instance_buf,
        }
    }

    /// Page bind group for textured glyph kinds, from the per-window
    /// snapshot taken at [`Self::flush`]. `Highlight` runs are painted
    /// from a frame-uniform-only pipeline and have no page binding —
    /// callers must check the run kind before invoking.
    pub(crate) fn page_bind_group(&self, kind: TextRunKind, page: u32) -> &wgpu::BindGroup {
        match kind {
            TextRunKind::Color => &self.color_page_bgs[page as usize],
            TextRunKind::Msdf => &self.msdf_page_bgs[page as usize],
            TextRunKind::Highlight => unreachable!("highlight runs carry no page binding"),
        }
    }
}

impl SharedTextInner {
    /// Mirror CPU atlas pages to GPU textures: create textures for new
    /// pages and upload the dirty regions. Called under the pool lock
    /// from each attached window's flush; dirty rects drain to whoever
    /// flushes first, and the upload is queue-ordered before that
    /// window's submit (later windows re-reference the same textures).
    fn flush_pages(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        // Colour pages.
        let color_dirty = self.atlas.take_dirty();
        while self.color_pages.len() < self.atlas.pages().len() {
            let i = self.color_pages.len();
            let page = &self.atlas.pages()[i];
            self.color_pages.push(create_color_page(
                device,
                &self.color_page_bind_layout,
                &self.color_sampler,
                page.width,
                page.height,
            ));
        }
        for (page_idx, rect) in color_dirty {
            let page = &self.atlas.pages()[page_idx];
            upload_color_region(queue, &self.color_pages[page_idx].texture, page, rect);
        }

        // MSDF pages.
        let msdf_dirty = self.msdf_atlas.take_dirty();
        while self.msdf_pages.len() < self.msdf_atlas.pages().len() {
            let i = self.msdf_pages.len();
            let page = &self.msdf_atlas.pages()[i];
            self.msdf_pages.push(create_msdf_page(
                device,
                &self.msdf_page_bind_layout,
                &self.msdf_sampler,
                page.width,
                page.height,
            ));
        }
        for (page_idx, rect) in msdf_dirty {
            let page = &self.msdf_atlas.pages()[page_idx];
            upload_msdf_region(queue, &self.msdf_pages[page_idx].texture, page, rect);
        }
    }

    /// See [`TextPaint::warm_default_glyphs`].
    pub(crate) fn warm_default_glyphs(&mut self) {
        const FAMILIES: &[FontFamily] = &[FontFamily::Inter, FontFamily::JetBrainsMono];
        let chars: Vec<char> = (0x20u32..=0x7Eu32).filter_map(char::from_u32).collect();
        self.warm_msdf_for_chars(&chars, FAMILIES);
    }

    /// Pre-rasterize the MSDF for each `(family, char)` pair. Looks
    /// up the first matching font in the fontdb per family at
    /// `Weight::NORMAL` — variable fonts return the same face for
    /// every weight, and MSDF keys are weight-independent at
    /// rasterization time, so a single warmup covers every weight the
    /// renderer later asks for.
    pub(crate) fn warm_msdf_for_chars(&mut self, chars: &[char], families: &[FontFamily]) {
        for family in families {
            let name = family.family_name();
            let font_id = self.atlas.font_system().db().query(&fontdb::Query {
                families: &[fontdb::Family::Name(name)],
                weight: fontdb::Weight::NORMAL,
                ..fontdb::Query::default()
            });
            let Some(font_id) = font_id else { continue };
            let face_index = self
                .atlas
                .font_system()
                .db()
                .face(font_id)
                .map(|f| f.index)
                .unwrap_or(0);
            let Some(font) = self
                .atlas
                .font_system_mut()
                .get_font(font_id, fontdb::Weight::NORMAL)
            else {
                continue;
            };
            let Ok(face) = Face::parse(font.data(), face_index) else {
                continue;
            };
            for &ch in chars {
                if let Some(glyph_id) = face.glyph_index(ch) {
                    let key = MsdfGlyphKey {
                        font: font_id,
                        glyph_id: glyph_id.0,
                    };
                    let _ = self.msdf_atlas.ensure(key, &face);
                }
            }
        }
    }
}

/// Resident-or-rasterize for one MSDF glyph against the shared pool.
fn ensure_msdf(
    inner: &mut SharedTextInner,
    key: MsdfGlyphKey,
    font_id: fontdb::ID,
    weight: fontdb::Weight,
) -> Option<MsdfSlot> {
    // touch (rather than slot) stamps the page as used this frame
    // so the LRU page recycler skips it.
    if let Some(slot) = inner.msdf_atlas.touch(key) {
        return Some(slot);
    }
    // Look up font bytes + face index, parse a ttf-parser Face,
    // then ask MsdfAtlas to rasterize. We can't borrow font_system
    // mutably (for get_font) and immutably (for db().face()) at
    // once, so we hop: get_font yields an Arc that owns the bytes,
    // then a separate immutable borrow for the face_index lookup.
    let font = inner.atlas.font_system_mut().get_font(font_id, weight)?;
    let face_index = inner.atlas.font_system().db().face(font_id)?.index;
    let face = Face::parse(font.data(), face_index).ok()?;
    inner.msdf_atlas.ensure(key, &face)
}

fn same_kind(a: TextRunKind, b: TextRunKind) -> bool {
    a == b
}

/// Build the colour-bitmap (`stock::text`) pipeline. Shared by `new` and
/// `set_target_format` so the descriptor stays a single source of truth —
/// only `target_format` varies across the two call sites.
fn build_color_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("stock::text"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(stock_wgsl::TEXT)),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("damascene_wgpu::text::color_pipeline"),
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
                    array_stride: std::mem::size_of::<ColorGlyphInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &COLOR_INSTANCE_ATTRS,
                },
            ],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(premultiplied_blend()),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: triangle_strip(),
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

/// Build the MSDF outline (`stock::text_msdf`) pipeline. See
/// [`build_color_pipeline`] for the new/set_target_format sharing rationale.
fn build_msdf_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("stock::text_msdf"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(stock_wgsl::TEXT_MSDF)),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("damascene_wgpu::text::msdf_pipeline"),
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
                    array_stride: std::mem::size_of::<MsdfGlyphInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &MSDF_INSTANCE_ATTRS,
                },
            ],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(premultiplied_blend()),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: triangle_strip(),
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

/// Build the inline-run highlight (`stock::text_highlight`) pipeline. See
/// [`build_color_pipeline`] for the new/set_target_format sharing rationale.
fn build_highlight_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("stock::text_highlight"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(stock_wgsl::TEXT_HIGHLIGHT)),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("damascene_wgpu::text::highlight_pipeline"),
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
                    array_stride: std::mem::size_of::<HighlightInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &HIGHLIGHT_INSTANCE_ATTRS,
                },
            ],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(premultiplied_blend()),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: triangle_strip(),
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

fn premultiplied_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

fn triangle_strip() -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleStrip,
        strip_index_format: None,
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: None,
        polygon_mode: wgpu::PolygonMode::Fill,
        unclipped_depth: false,
        conservative: false,
    }
}

fn create_color_page(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    width: u32,
    height: u32,
) -> PageTexture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("damascene_wgpu::text::color_page"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("damascene_wgpu::text::color_page_bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    PageTexture {
        texture,
        bind_group,
    }
}

fn create_msdf_page(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    width: u32,
    height: u32,
) -> PageTexture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("damascene_wgpu::text::msdf_page"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // MSDF distance encodes per-channel; storing them in a *linear*
        // texture avoids the sRGB EOTF being applied to distance bytes.
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("damascene_wgpu::text::msdf_page_bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    PageTexture {
        texture,
        bind_group,
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
    ) -> std::ops::Range<usize> {
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
    ) -> std::ops::Range<usize> {
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

fn upload_color_region(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    page: &AtlasPage,
    rect: AtlasRect,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }
    let bpp = ATLAS_BYTES_PER_PIXEL as usize;
    let row_bytes = rect.w as usize * bpp;
    let mut bytes = Vec::with_capacity(row_bytes * rect.h as usize);
    for row in 0..rect.h {
        let y = rect.y + row;
        let start = (y as usize * page.width as usize + rect.x as usize) * bpp;
        let end = start + row_bytes;
        bytes.extend_from_slice(&page.pixels[start..end]);
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: rect.x,
                y: rect.y,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(rect.w * ATLAS_BYTES_PER_PIXEL),
            rows_per_image: Some(rect.h),
        },
        wgpu::Extent3d {
            width: rect.w,
            height: rect.h,
            depth_or_array_layers: 1,
        },
    );
}

fn upload_msdf_region(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    page: &MsdfAtlasPage,
    rect: MsdfRect,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }
    let bpp = MSDF_BYTES_PER_PIXEL as usize;
    let row_bytes = rect.w as usize * bpp;
    let mut bytes = Vec::with_capacity(row_bytes * rect.h as usize);
    for row in 0..rect.h {
        let y = rect.y + row;
        let start = (y as usize * page.width as usize + rect.x as usize) * bpp;
        let end = start + row_bytes;
        bytes.extend_from_slice(&page.pixels[start..end]);
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: rect.x,
                y: rect.y,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(rect.w * MSDF_BYTES_PER_PIXEL),
            rows_per_image: Some(rect.h),
        },
        wgpu::Extent3d {
            width: rect.w,
            height: rect.h,
            depth_or_array_layers: 1,
        },
    );
}
