//! GPU vector-icon rendering.
//!
//! Two paths sharing one [`IconPaint`]:
//!
//! - **MSDF**: pre-rasterised once per `(icon source, stroke_width)`
//!   into an MTSDF atlas and rendered through `stock::text_msdf` (one
//!   quad per icon). Used for the default `Flat` material, where
//!   coverage is the only thing the fragment shader needs from the
//!   icon geometry. App-supplied [`damascene_core::SvgIcon`]s share the
//!   same path, keyed on their content hash.
//! - **Tessellated**: lyon-tessellated triangles with analytic-AA
//!   fringes, drawn through the local-coord `stock::vector*` shaders.
//!   Kept for `Relief` / `Glass` materials which need per-fragment
//!   view-box coordinates the MSDF path doesn't carry. Authored-paint
//!   custom SVG icons and painted app vectors also use this path so
//!   per-path colour and gradients survive.
//!
//! Each [`IconRun`] carries a `kind` so the renderer knows which path
//! to draw it through.
//!
//! Built-in icons are still parsed from SVG through `usvg` into Damascene's
//! backend-agnostic vector IR; explicit mask draws run that IR through
//! kurbo (stroke→fill) and fdsm (MTSDF generation), while painted draws
//! run through lyon.

use std::borrow::Cow;
use std::ops::Range;

use damascene_core::color::ColorSpace;
use damascene_core::icons::msdf_atlas::{
    DEFAULT_PX_PER_UNIT, DEFAULT_SPREAD, IconMsdfAtlas, IconMsdfPage, IconMsdfSlot, IconRect,
};
use damascene_core::icons::svg::{IconSource, SvgIconPaintMode};
use damascene_core::paint::{
    DEFAULT_WORKING_COLOR_SPACE, IconRun, IconRunKind, PhysicalScissor, rgba_f32_in,
};
use damascene_core::shader::stock_wgsl;
use damascene_core::tree::{Color, Rect};
use damascene_core::vector::{
    GRADIENT_RAMP_WIDTH, IconMaterial, MAX_FRAME_GRADIENTS, VectorAsset, VectorGradientFrame,
    VectorGradientGpuParams, VectorMeshOptions, VectorMeshVertex, VectorRenderMode,
    append_vector_asset_mesh,
};

use bytemuck::{Pod, Zeroable};

const INITIAL_VERTEX_CAPACITY: usize = 1024;
const INITIAL_INSTANCE_CAPACITY: usize = 256;

const TESS_VERTEX_ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x2, // position in logical px
    1 => Float32x2, // local SVG/viewBox coordinate
    2 => Float32x4, // linear rgba
    3 => Float32x4, // vector metadata
];

const MSDF_INSTANCE_ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    1 => Float32x4, // rect  (xy = top-left logical px, zw = size logical px)
    2 => Float32x4, // uv    (xy = uv 0..1, zw = uv size 0..1)
    3 => Float32x4, // color (linear rgba 0..1)
    4 => Float32x4, // params (x = atlas-space spread, y/z/w reserved)
];

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct MsdfIconInstance {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: [f32; 4],
    pub params: [f32; 4],
}

struct MsdfPageTexture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

pub(crate) struct IconPaint {
    // Tessellated path (legacy + non-flat materials).
    tess_vertices: Vec<VectorMeshVertex>,
    tess_vertex_buf: wgpu::Buffer,
    tess_vertex_capacity: usize,
    flat_pipeline: wgpu::RenderPipeline,
    relief_pipeline: wgpu::RenderPipeline,
    glass_pipeline: wgpu::RenderPipeline,

    // Fragment-stage gradient table (issues #140/#141): params uniform +
    // baked ramp rows, bound at group(1) of the tess pipelines. See
    // `VectorGradientFrame` for the contract.
    gradient_frame: VectorGradientFrame,
    gradient_uniform_buf: wgpu::Buffer,
    gradient_ramp_tex: wgpu::Texture,
    gradient_bind_group: wgpu::BindGroup,

    // MSDF path (Flat material).
    msdf_atlas: IconMsdfAtlas,
    msdf_pages: Vec<MsdfPageTexture>,
    msdf_instances: Vec<MsdfIconInstance>,
    msdf_instance_buf: wgpu::Buffer,
    msdf_instance_capacity: usize,
    msdf_pipeline: wgpu::RenderPipeline,
    msdf_page_bind_layout: wgpu::BindGroupLayout,
    msdf_sampler: wgpu::Sampler,

    // Pipeline layouts + sample count retained so the four
    // swapchain-format-bound pipelines above (flat/relief/glass tess + MSDF)
    // can be rebuilt in place on a surface-format renegotiation
    // (`set_target_format`). The page bind-group layout is unchanged, so the
    // cached MSDF page bind groups stay valid.
    tess_pipeline_layout: wgpu::PipelineLayout,
    msdf_pipeline_layout: wgpu::PipelineLayout,
    sample_count: u32,

    runs: Vec<IconRun>,
    material: IconMaterial,
    /// Working color space icon colors are converted into. Kept in sync
    /// with the owning `Runner` via `set_working_color_space`.
    working_color_space: ColorSpace,
}

impl IconPaint {
    pub(crate) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        sample_count: u32,
        frame_bind_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        // ---- Tess pipelines ----
        let tess_vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("damascene_wgpu::icon::tess_vertex_buf"),
            size: (INITIAL_VERTEX_CAPACITY * std::mem::size_of::<VectorMeshVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let gradient_bind_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("damascene_wgpu::icon::gradient_bind_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let tess_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("damascene_wgpu::icon::tess_pipeline_layout"),
            bind_group_layouts: &[Some(frame_bind_layout), Some(&gradient_bind_layout)],
            immediate_size: 0,
        });
        let flat_pipeline = build_tess_pipeline(
            device,
            &tess_pipeline_layout,
            target_format,
            sample_count,
            "stock::vector",
            stock_wgsl::VECTOR,
        );
        let relief_pipeline = build_tess_pipeline(
            device,
            &tess_pipeline_layout,
            target_format,
            sample_count,
            "stock::vector_relief",
            stock_wgsl::VECTOR_RELIEF,
        );
        let glass_pipeline = build_tess_pipeline(
            device,
            &tess_pipeline_layout,
            target_format,
            sample_count,
            "stock::vector_glass",
            stock_wgsl::VECTOR_GLASS,
        );

        // ---- MSDF pipeline ----
        let msdf_page_bind_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("damascene_wgpu::icon::msdf_page_bind_layout"),
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
            });

        let msdf_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("damascene_wgpu::icon::msdf_pipeline_layout"),
            bind_group_layouts: &[Some(frame_bind_layout), Some(&msdf_page_bind_layout)],
            immediate_size: 0,
        });
        let msdf_pipeline =
            build_msdf_pipeline(device, &msdf_pipeline_layout, target_format, sample_count);

        let msdf_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("damascene_wgpu::icon::msdf_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let msdf_instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("damascene_wgpu::icon::msdf_instance_buf"),
            size: (INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<MsdfIconInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // ---- Gradient table (fixed-size, zero-initialized) ----
        let gradient_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("damascene_wgpu::icon::gradient_uniform_buf"),
            size: (MAX_FRAME_GRADIENTS * std::mem::size_of::<VectorGradientGpuParams>()) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let gradient_ramp_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("damascene_wgpu::icon::gradient_ramp_tex"),
            size: wgpu::Extent3d {
                width: GRADIENT_RAMP_WIDTH as u32,
                height: MAX_FRAME_GRADIENTS as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let gradient_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("damascene_wgpu::icon::gradient_bind_group"),
            layout: &gradient_bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: gradient_uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &gradient_ramp_tex.create_view(&wgpu::TextureViewDescriptor::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    // Same clamp + bilinear config the ramp rows need.
                    resource: wgpu::BindingResource::Sampler(&msdf_sampler),
                },
            ],
        });

        Self {
            tess_vertices: Vec::with_capacity(INITIAL_VERTEX_CAPACITY),
            tess_vertex_buf,
            tess_vertex_capacity: INITIAL_VERTEX_CAPACITY,
            flat_pipeline,
            relief_pipeline,
            glass_pipeline,
            gradient_frame: VectorGradientFrame::new(),
            gradient_uniform_buf,
            gradient_ramp_tex,
            gradient_bind_group,
            msdf_atlas: IconMsdfAtlas::new(DEFAULT_PX_PER_UNIT, DEFAULT_SPREAD),
            msdf_pages: Vec::new(),
            msdf_instances: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY),
            msdf_instance_buf,
            msdf_instance_capacity: INITIAL_INSTANCE_CAPACITY,
            msdf_pipeline,
            msdf_page_bind_layout,
            msdf_sampler,
            tess_pipeline_layout,
            msdf_pipeline_layout,
            sample_count,
            runs: Vec::new(),
            material: IconMaterial::Flat,
            working_color_space: DEFAULT_WORKING_COLOR_SPACE,
        }
    }

    pub(crate) fn set_material(&mut self, material: IconMaterial) {
        self.material = material;
    }

    /// Rebuild the four swapchain-format-bound pipelines (flat/relief/glass
    /// tess + MSDF) for a new target format, preserving the MSDF atlas, page
    /// textures, vertex/instance buffers, and sampler. Called by
    /// `Runner::set_target_format`. The pipeline layouts and page bind-group
    /// layout are unchanged, so cached page bind groups stay valid.
    pub(crate) fn set_target_format(
        &mut self,
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
    ) {
        self.flat_pipeline = build_tess_pipeline(
            device,
            &self.tess_pipeline_layout,
            target_format,
            self.sample_count,
            "stock::vector",
            stock_wgsl::VECTOR,
        );
        self.relief_pipeline = build_tess_pipeline(
            device,
            &self.tess_pipeline_layout,
            target_format,
            self.sample_count,
            "stock::vector_relief",
            stock_wgsl::VECTOR_RELIEF,
        );
        self.glass_pipeline = build_tess_pipeline(
            device,
            &self.tess_pipeline_layout,
            target_format,
            self.sample_count,
            "stock::vector_glass",
            stock_wgsl::VECTOR_GLASS,
        );
        self.msdf_pipeline = build_msdf_pipeline(
            device,
            &self.msdf_pipeline_layout,
            target_format,
            self.sample_count,
        );
    }

    /// Update the working color space subsequent icon color packing
    /// converts into. Called by `Runner::set_working_color_space`.
    pub(crate) fn set_working_color_space(&mut self, space: ColorSpace) {
        self.working_color_space = space;
    }

    pub(crate) fn material(&self) -> IconMaterial {
        self.material
    }

    pub(crate) fn frame_begin(&mut self) {
        self.tess_vertices.clear();
        self.msdf_instances.clear();
        self.runs.clear();
        self.gradient_frame.begin(self.working_color_space);
    }

    pub(crate) fn record(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        source: &IconSource,
        color: Color,
        stroke_width: f32,
    ) -> Range<usize> {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            let start = self.runs.len();
            return start..start;
        }
        let start = self.runs.len();

        let use_msdf = matches!(self.material, IconMaterial::Flat)
            && matches!(source.paint_mode(), SvgIconPaintMode::CurrentColorMask);

        if use_msdf {
            if let Some(slot) = self.msdf_atlas.ensure(source, stroke_width) {
                let (page_w, page_h) = self.msdf_page_dims(slot.page);
                let instance = msdf_instance_for_icon(
                    rect,
                    color,
                    &slot,
                    page_w,
                    page_h,
                    self.working_color_space,
                );
                let first = self.msdf_instances.len() as u32;
                self.msdf_instances.push(instance);
                self.runs.push(IconRun {
                    kind: IconRunKind::Msdf,
                    scissor,
                    first,
                    count: 1,
                    page: slot.page,
                    material: IconMaterial::Flat,
                });
            }
        } else {
            let material = self.material;
            let asset = source.vector_asset();
            let first = self.tess_vertices.len() as u32;
            let mesh_run = append_vector_asset_mesh(
                asset,
                VectorMeshOptions::icon(rect, color, stroke_width, self.working_color_space),
                &mut self.tess_vertices,
                Some(&mut self.gradient_frame),
            );
            if mesh_run.count > 0 {
                self.runs.push(IconRun {
                    kind: IconRunKind::Tess,
                    scissor,
                    first,
                    count: mesh_run.count,
                    page: 0,
                    material,
                });
            }
        }
        start..self.runs.len()
    }

    /// Record an app-supplied [`VectorAsset`] for paint. The render mode
    /// is chosen by core/app code, not inferred from vector contents.
    pub(crate) fn record_vector(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        asset: &VectorAsset,
        render_mode: VectorRenderMode,
    ) -> Range<usize> {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            let start = self.runs.len();
            return start..start;
        }
        let start = self.runs.len();

        match render_mode {
            VectorRenderMode::Mask { color } => {
                if let Some(slot) = self.msdf_atlas.ensure_vector_asset(asset) {
                    let (page_w, page_h) = self.msdf_page_dims(slot.page);
                    let instance = msdf_instance_for_icon(
                        rect,
                        color,
                        &slot,
                        page_w,
                        page_h,
                        self.working_color_space,
                    );
                    let first = self.msdf_instances.len() as u32;
                    self.msdf_instances.push(instance);
                    self.runs.push(IconRun {
                        kind: IconRunKind::Msdf,
                        scissor,
                        first,
                        count: 1,
                        page: slot.page,
                        material: IconMaterial::Flat,
                    });
                }
            }
            VectorRenderMode::Painted => {
                // Tess fallback. Painted assets preserve per-path colours,
                // gradients, and currentColor paint via per-vertex attributes.
                let first = self.tess_vertices.len() as u32;
                let mesh_run = append_vector_asset_mesh(
                    asset,
                    VectorMeshOptions::icon(
                        rect,
                        Color::srgb_u8(255, 255, 255),
                        1.0,
                        self.working_color_space,
                    ),
                    &mut self.tess_vertices,
                    Some(&mut self.gradient_frame),
                );
                if mesh_run.count > 0 {
                    self.runs.push(IconRun {
                        kind: IconRunKind::Tess,
                        scissor,
                        first,
                        count: mesh_run.count,
                        page: 0,
                        material: IconMaterial::Flat,
                    });
                }
            }
        }
        start..self.runs.len()
    }

    /// `(width, height)` of the CPU-side atlas page for `page_idx`.
    /// The GPU mirror may not exist yet (it's created at flush time);
    /// the UV computation only needs the page extent, not the texture.
    fn msdf_page_dims(&self, page_idx: u32) -> (u32, u32) {
        let page = self
            .msdf_atlas
            .page(page_idx)
            .expect("freshly-ensured slot references a missing atlas page");
        (page.width, page.height)
    }

    pub(crate) fn flush(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        // Tess vertex buffer.
        if self.tess_vertices.len() > self.tess_vertex_capacity {
            let new_cap = self.tess_vertices.len().next_power_of_two();
            self.tess_vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("damascene_wgpu::icon::tess_vertex_buf (resized)"),
                size: (new_cap * std::mem::size_of::<VectorMeshVertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.tess_vertex_capacity = new_cap;
        }
        if !self.tess_vertices.is_empty() {
            queue.write_buffer(
                &self.tess_vertex_buf,
                0,
                bytemuck::cast_slice(&self.tess_vertices),
            );
        }

        // Gradient table: used params prefix + the baked ramp rows. The
        // buffer/texture are fixed-size (MAX_FRAME_GRADIENTS), so no
        // reallocation and the bind group stays valid.
        if !self.gradient_frame.is_empty() {
            queue.write_buffer(
                &self.gradient_uniform_buf,
                0,
                bytemuck::cast_slice(self.gradient_frame.params()),
            );
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.gradient_ramp_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(self.gradient_frame.ramp_data()),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some((GRADIENT_RAMP_WIDTH * 8) as u32),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: GRADIENT_RAMP_WIDTH as u32,
                    height: self.gradient_frame.slot_count(),
                    depth_or_array_layers: 1,
                },
            );
        }

        // MSDF pages: create GPU textures for any newly-allocated atlas
        // pages, then upload dirty regions.
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
        for (page_idx, rect) in self.msdf_atlas.take_dirty() {
            let page = &self.msdf_atlas.pages()[page_idx];
            upload_msdf_region(queue, &self.msdf_pages[page_idx].texture, page, rect);
        }

        // MSDF instance buffer.
        if self.msdf_instances.len() > self.msdf_instance_capacity {
            let new_cap = self.msdf_instances.len().next_power_of_two();
            self.msdf_instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("damascene_wgpu::icon::msdf_instance_buf (resized)"),
                size: (new_cap * std::mem::size_of::<MsdfIconInstance>()) as u64,
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
    }

    pub(crate) fn run(&self, index: usize) -> IconRun {
        self.runs[index]
    }

    pub(crate) fn tess_pipeline(&self, material: IconMaterial) -> &wgpu::RenderPipeline {
        match material {
            IconMaterial::Flat => &self.flat_pipeline,
            IconMaterial::Relief => &self.relief_pipeline,
            IconMaterial::Glass => &self.glass_pipeline,
        }
    }

    pub(crate) fn tess_vertex_buf(&self) -> &wgpu::Buffer {
        &self.tess_vertex_buf
    }

    /// Gradient table bind group for group(1) of the tess pipelines.
    pub(crate) fn gradient_bind_group(&self) -> &wgpu::BindGroup {
        &self.gradient_bind_group
    }

    pub(crate) fn msdf_pipeline(&self) -> &wgpu::RenderPipeline {
        &self.msdf_pipeline
    }

    pub(crate) fn msdf_instance_buf(&self) -> &wgpu::Buffer {
        &self.msdf_instance_buf
    }

    pub(crate) fn msdf_page_bind_group(&self, page: u32) -> &wgpu::BindGroup {
        &self.msdf_pages[page as usize].bind_group
    }
}

fn msdf_instance_for_icon(
    rect: Rect,
    color: Color,
    slot: &IconMsdfSlot,
    page_w: u32,
    page_h: u32,
    working_color_space: ColorSpace,
) -> MsdfIconInstance {
    // Expand the destination rect outward by the spread margin in
    // logical px so the full atlas slot (including the SDF skirt) has
    // somewhere to land — without this, the AA fringe is clipped by the
    // unit-quad edge.
    let [_, _, vw, vh] = slot.view_box;
    let logical_per_unit_x = rect.w / vw.max(0.001);
    let logical_per_unit_y = rect.h / vh.max(0.001);
    let spread_x = slot.spread * logical_per_unit_x / slot.px_per_unit.max(0.001);
    let spread_y = slot.spread * logical_per_unit_y / slot.px_per_unit.max(0.001);

    let bx = rect.x - spread_x;
    let by = rect.y - spread_y;
    let bw = rect.w + 2.0 * spread_x;
    let bh = rect.h + 2.0 * spread_y;

    let pw = page_w as f32;
    let ph = page_h as f32;
    let uv = [
        slot.rect.x as f32 / pw,
        slot.rect.y as f32 / ph,
        slot.rect.w as f32 / pw,
        slot.rect.h as f32 / ph,
    ];

    MsdfIconInstance {
        rect: [bx, by, bw, bh],
        uv,
        color: rgba_f32_in(color, working_color_space),
        params: [slot.spread, 0.0, 0.0, 0.0],
    }
}

fn build_tess_pipeline(
    device: &wgpu::Device,
    pipeline_layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
    sample_count: u32,
    label: &'static str,
    wgsl: &'static str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(wgsl)),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<VectorMeshVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &TESS_VERTEX_ATTRS,
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                // The vector shaders output premultiplied colour
                // (`paint.rgb * paint.a`), so blend with One — SrcAlpha
                // here would double-multiply translucent paint
                // (stop-opacity/fill-opacity < 1, Glass alpha).
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
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

/// Build the icon MSDF pipeline (`stock::text_msdf`). Shared by `new` and
/// `set_target_format` so the descriptor stays a single source of truth —
/// only `target_format` varies across the two call sites.
fn build_msdf_pipeline(
    device: &wgpu::Device,
    pipeline_layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("stock::text_msdf (icon)"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(stock_wgsl::TEXT_MSDF)),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("damascene_wgpu::icon::msdf_pipeline"),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[
                Some(wgpu::VertexBufferLayout {
                    array_stride: (2 * std::mem::size_of::<f32>()) as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                    }],
                }),
                Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<MsdfIconInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &MSDF_INSTANCE_ATTRS,
                }),
            ],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                // text_msdf outputs premultiplied colour; pair with
                // a premultiplied-alpha blend.
                blend: Some(wgpu::BlendState {
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
                }),
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

fn create_msdf_page(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    width: u32,
    height: u32,
) -> MsdfPageTexture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("damascene_wgpu::icon::msdf_page"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("damascene_wgpu::icon::msdf_page_bg"),
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
    MsdfPageTexture {
        texture,
        bind_group,
    }
}

fn upload_msdf_region(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    page: &IconMsdfPage,
    rect: IconRect,
) {
    // Pages are RGBA8 — 4 bytes per pixel.
    let bpp: u32 = 4;
    let row_bytes = page.width as usize * bpp as usize;
    let dst_x = rect.x;
    let dst_y = rect.y;
    let mut staging = vec![0u8; (rect.w * rect.h * bpp) as usize];
    for r in 0..rect.h as usize {
        let src_off = (rect.y as usize + r) * row_bytes + rect.x as usize * bpp as usize;
        let dst_off = r * (rect.w * bpp) as usize;
        let len = (rect.w * bpp) as usize;
        staging[dst_off..dst_off + len].copy_from_slice(&page.pixels[src_off..src_off + len]);
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: dst_x,
                y: dst_y,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        &staging,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(rect.w * bpp),
            rows_per_image: Some(rect.h),
        },
        wgpu::Extent3d {
            width: rect.w,
            height: rect.h,
            depth_or_array_layers: 1,
        },
    );
}
