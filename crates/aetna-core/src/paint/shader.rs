//! Shader handles, uniform values, and bindings.
//!
//! ## Stock shader source
//!
//! WGSL source for stock shaders is exposed under [`stock_wgsl`] so
//! backend crates can `include_str!`-equivalent it without reaching
//! across crate directories. See `crates/aetna-core/shaders/`.
//!
//! Sits between the grammar layer (where users write `.fill(c)`,
//! `.radius(r)`) and the renderer (which consumes typed [`crate::DrawOp`]s with
//! shader handles + uniform blocks).
//!
//! The grammar layer doesn't speak shaders directly. The renderer walks
//! the tree and constructs a [`ShaderBinding`] per visual fact, defaulting
//! to a stock shader (e.g. [`StockShader::RoundedRect`] for rect-shaped
//! surfaces). A user crate can override that default by setting
//! [`crate::tree::El::shader_override`].
//!
//! Stock shaders are pre-compiled wgsl modules shipped with the crate.
//! Custom shaders are user-registered wgsl source identified by name.
//! The SVG fallback renderer interprets stock shaders best-effort and
//! emits placeholder rects for custom ones.
//!
//! See `docs/SHADER_VISION.md` for the rendering-layer contract.
//!
//! # Uniform packing
//!
//! [`UniformBlock`] is a `BTreeMap` keyed by `&'static str` for stable
//! iteration order — important so that the bundle's
//! `shader_manifest.txt` artifact is deterministic and grep-friendly.
//! Backend runners pack the block to the target GPU ABI using their
//! per-shader layout metadata. Bundle/SVG paths consume the typed map
//! directly when producing diagnostics.
//!
//! # Stock-shader status
//!
//! Focus indicators ride on each focusable node's own `RoundedRect` quad via
//! `focus_color`/`focus_width` uniforms. Most surface variation should remain
//! uniform/theme driven rather than creating more stock shaders.

use std::collections::BTreeMap;

use crate::tree::Color;

/// Where a draw op's pixels come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShaderHandle {
    Stock(StockShader),
    /// User-registered shader. The string is the name passed to the backend
    /// runner at host-integration time.
    Custom(&'static str),
}

impl ShaderHandle {
    pub fn name(&self) -> String {
        match self {
            ShaderHandle::Stock(s) => s.name().to_string(),
            ShaderHandle::Custom(n) => format!("custom::{n}"),
        }
    }
}

/// Shipped shader inventory. See `docs/SHADER_VISION.md` for the shader model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StockShader {
    /// Flat colored rect. Fallback / debug.
    SolidQuad,
    /// Fill + stroke + radius + shadow + focus ring. The workhorse —
    /// handles ~80% of UI surfaces. Focus indicator is a uniform on
    /// this shader, not a separate pipeline (see `widget_kit.md`).
    RoundedRect,
    /// Alpha-mask glyph rendering. Backends sample per-glyph bitmaps
    /// from a [`crate::text::atlas::GlyphAtlas`] page texture and tint
    /// by per-glyph color. The historical `TextSdf` name was aspirational;
    /// the actual rasterization is alpha-coverage via swash.
    Text,
    /// Antialiased 1px line.
    DividerLine,
    /// Per-image raster sampling. Backend binds a per-image texture at
    /// group 1 and the fragment shader composes `sampled * tint` with
    /// rounded-corner AA. See `crate::image::Image` for the data side.
    Image,
    /// Indeterminate loading spinner — circular SDF arc swept around a
    /// dim track, animated by `frame.time`. Continuous: any node bound
    /// to this shader keeps `needs_redraw` set so the host idle loop
    /// keeps ticking.
    Spinner,
    /// Pulsing loading placeholder — a rounded rect with a cosine
    /// alpha breathe (0.5 → 1.0 → 0.5 over 2 s by default) matching
    /// shadcn's `animate-pulse`. Continuous.
    Skeleton,
    /// Indeterminate linear progress — a track with a small bar
    /// sliding left-to-right on loop, for in-line "still working…"
    /// feedback when no completion ratio is known. Continuous.
    ProgressIndeterminate,
}

impl StockShader {
    pub fn name(self) -> &'static str {
        match self {
            StockShader::SolidQuad => "stock::solid_quad",
            StockShader::RoundedRect => "stock::rounded_rect",
            StockShader::Text => "stock::text",
            StockShader::DividerLine => "stock::divider_line",
            StockShader::Image => "stock::image",
            StockShader::Spinner => "stock::spinner",
            StockShader::Skeleton => "stock::skeleton",
            StockShader::ProgressIndeterminate => "stock::progress_indeterminate",
        }
    }

    /// Whether this shader's output depends on `frame.time`, i.e. the
    /// host must keep redrawing for it to animate. Read by
    /// [`crate::runtime::RunnerCore::prepare_layout`] when computing
    /// `needs_redraw`.
    pub fn is_continuous(self) -> bool {
        matches!(
            self,
            StockShader::Spinner | StockShader::Skeleton | StockShader::ProgressIndeterminate
        )
    }
}

/// A single uniform's value. Keep small and concrete; this is the wire
/// format between the grammar layer and the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UniformValue {
    F32(f32),
    Vec2([f32; 2]),
    Vec4([f32; 4]),
    Color(Color),
    Bool(bool),
}

impl UniformValue {
    /// Compact form for tree dump / shader manifest.
    pub fn debug_short(&self) -> String {
        match self {
            UniformValue::F32(v) => format!("{v:.2}"),
            UniformValue::Vec2([x, y]) => format!("({x:.2},{y:.2})"),
            UniformValue::Vec4([x, y, z, w]) => format!("({x:.2},{y:.2},{z:.2},{w:.2})"),
            UniformValue::Color(c) => match c.token {
                Some(name) => name.to_string(),
                None => format!("rgba({},{},{},{})", c.r, c.g, c.b, c.a),
            },
            UniformValue::Bool(b) => b.to_string(),
        }
    }
}

/// Named uniform values for a single draw. `BTreeMap` for deterministic
/// iteration in artifacts.
pub type UniformBlock = BTreeMap<&'static str, UniformValue>;

/// A shader handle plus the uniforms to bind for one draw.
#[derive(Clone, Debug)]
pub struct ShaderBinding {
    pub handle: ShaderHandle,
    pub uniforms: UniformBlock,
}

impl ShaderBinding {
    pub fn stock(shader: StockShader) -> Self {
        Self {
            handle: ShaderHandle::Stock(shader),
            uniforms: UniformBlock::new(),
        }
    }
    pub fn custom(name: &'static str) -> Self {
        Self {
            handle: ShaderHandle::Custom(name),
            uniforms: UniformBlock::new(),
        }
    }
    pub fn with(mut self, key: &'static str, value: UniformValue) -> Self {
        self.uniforms.insert(key, value);
        self
    }
    pub fn set(&mut self, key: &'static str, value: UniformValue) {
        self.uniforms.insert(key, value);
    }

    // Typed sugar for the common cases — saves the user from typing
    // `UniformValue::Color(...)` at every call site.

    pub fn color(self, key: &'static str, c: Color) -> Self {
        self.with(key, UniformValue::Color(c))
    }
    pub fn f32(self, key: &'static str, v: f32) -> Self {
        self.with(key, UniformValue::F32(v))
    }
    pub fn vec4(self, key: &'static str, v: [f32; 4]) -> Self {
        self.with(key, UniformValue::Vec4(v))
    }
}

/// WGSL source for stock shaders. Backend crates compile these into
/// pipelines; the source lives here so the asset shipping is centralised.
pub mod stock_wgsl {
    pub const ROUNDED_RECT: &str = include_str!("../../shaders/rounded_rect.wgsl");
    pub const TEXT: &str = include_str!("../../shaders/text.wgsl");
    pub const TEXT_MSDF: &str = include_str!("../../shaders/text_msdf.wgsl");
    pub const TEXT_HIGHLIGHT: &str = include_str!("../../shaders/text_highlight.wgsl");
    pub const ICON_LINE: &str = include_str!("../../shaders/icon_line.wgsl");
    pub const VECTOR: &str = include_str!("../../shaders/vector.wgsl");
    pub const VECTOR_RELIEF: &str = include_str!("../../shaders/vector_relief.wgsl");
    pub const VECTOR_GLASS: &str = include_str!("../../shaders/vector_glass.wgsl");
    pub const IMAGE: &str = include_str!("../../shaders/image.wgsl");
    pub const SURFACE: &str = include_str!("../../shaders/surface.wgsl");
    pub const SPINNER: &str = include_str!("../../shaders/spinner.wgsl");
    pub const SKELETON: &str = include_str!("../../shaders/skeleton.wgsl");
    pub const PROGRESS_INDETERMINATE: &str =
        include_str!("../../shaders/progress_indeterminate.wgsl");

    // Scene3D pipelines. Self-contained (their own `@group(0)` uniform,
    // not the stock frame uniforms); shared by every backend's scene
    // renderer via naga, like the other stock sources.
    pub const SCENE_POINT: &str = include_str!("../../shaders/scene_point.wgsl");
    pub const SCENE_LINE: &str = include_str!("../../shaders/scene_line.wgsl");
    pub const SCENE_MESH: &str = include_str!("../../shaders/scene_mesh.wgsl");
}
