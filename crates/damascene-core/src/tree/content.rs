//! Content-related [`El`] modifiers: text runs, icon source, and raster image source.

use crate::image::{Image, ImageFit};

use super::layout_types::Size;
use super::node::El;
use super::semantics::Kind;
use super::text_types::{FontFamily, FontWeight, TextAlign, TextOverflow, TextRole, TextWrap};
use crate::color::Color;

impl El {
    // ---- Text-bearing ----
    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.text = Some(t.into());
        self
    }

    pub fn text_color(mut self, c: Color) -> Self {
        self.text_color = Some(c);
        self
    }

    pub fn text_align(mut self, align: TextAlign) -> Self {
        self.text_align = align;
        self
    }

    pub fn center_text(self) -> Self {
        self.text_align(TextAlign::Center)
    }

    pub fn end_text(self) -> Self {
        self.text_align(TextAlign::End)
    }

    pub fn text_wrap(mut self, wrap: TextWrap) -> Self {
        self.text_wrap = wrap;
        self
    }

    pub fn wrap_text(self) -> Self {
        self.text_wrap(TextWrap::Wrap)
    }

    pub fn nowrap_text(self) -> Self {
        self.text_wrap(TextWrap::NoWrap)
    }

    pub fn text_overflow(mut self, overflow: TextOverflow) -> Self {
        self.text_overflow = overflow;
        self
    }

    pub fn ellipsis(self) -> Self {
        self.text_overflow(TextOverflow::Ellipsis)
    }

    pub fn max_lines(mut self, lines: usize) -> Self {
        self.text_max_lines = Some(lines.max(1));
        self
    }

    pub fn font_size(mut self, s: f32) -> Self {
        self.font_size = s;
        self.line_height = crate::tokens::line_height_for_size(s);
        self
    }

    pub fn line_height(mut self, h: f32) -> Self {
        self.line_height = h.max(1.0);
        self
    }

    pub fn font_weight(mut self, w: FontWeight) -> Self {
        self.font_weight = w;
        self
    }

    pub fn font_family(mut self, family: FontFamily) -> Self {
        self.font_family = family;
        self.explicit_font_family = true;
        self
    }

    pub fn inter(self) -> Self {
        self.font_family(FontFamily::Inter)
    }

    pub fn roboto(self) -> Self {
        self.font_family(FontFamily::Roboto)
    }

    /// Override the monospace face used when this node renders as code
    /// (`font_mono = true`, `TextRole::Code`, or any descendant that
    /// inherits the value through theme propagation). Setting this
    /// pins the node — theme `with_mono_font_family(...)` no longer
    /// stamps over it.
    pub fn mono_font_family(mut self, family: FontFamily) -> Self {
        self.mono_font_family = family;
        self.explicit_mono_font_family = true;
        self
    }

    /// Pin this node's monospace face to JetBrains Mono. Convenience
    /// shorthand for `.mono_font_family(FontFamily::JetBrainsMono)`.
    pub fn jetbrains_mono(self) -> Self {
        self.mono_font_family(FontFamily::JetBrainsMono)
    }

    /// Set the icon for this element to either a built-in [`crate::IconName`],
    /// an app-supplied [`crate::SvgIcon`], or a string-typed name from
    /// the built-in vocabulary.
    pub fn icon_source(mut self, source: impl crate::icons::svg::IntoIconSource) -> Self {
        self.icon = Some(source.into_icon_source());
        self
    }

    /// Convenience alias for [`Self::icon_source`] preserved for call
    /// sites that want the historical name.
    pub fn icon_name(self, source: impl crate::icons::svg::IntoIconSource) -> Self {
        self.icon_source(source)
    }

    pub fn icon_stroke_width(mut self, width: f32) -> Self {
        self.icon_stroke_width = width.max(0.25);
        self
    }

    /// Set the icon glyph metric (`font_size` + `line_height`) for this
    /// element, and — when this element *is* an icon — its layout box.
    ///
    /// Kind-aware: width/height are only assigned when `self.kind` is
    /// `Kind::Custom("icon")`. On container kinds like
    /// [`crate::button_with_icon`] or `Kind::Custom("icon_button")`,
    /// only `font_size` / `line_height` are set — chaining
    /// `.icon_size(...)` no longer collapses the outer rect.
    ///
    /// Propagates to direct children whose `kind` is `Kind::Custom("icon")`
    /// so a caller's `.icon_size(...)` on a container widget resizes the
    /// inner icon child too. Containers that hold the icon deeper than
    /// one level (or behind a layout wrapper) need to set the icon size
    /// at the icon site directly.
    pub fn icon_size(mut self, size: f32) -> Self {
        let size = size.max(1.0);
        self.font_size = size;
        self.line_height = size;
        if matches!(&self.kind, Kind::Custom(name) if *name == "icon") {
            self.width = Size::Fixed(size);
            self.height = Size::Fixed(size);
            self.explicit_width = true;
            self.explicit_height = true;
        }
        for child in self.children.iter_mut() {
            if matches!(&child.kind, Kind::Custom(name) if *name == "icon") {
                child.font_size = size;
                child.line_height = size;
                child.width = Size::Fixed(size);
                child.height = Size::Fixed(size);
                child.explicit_width = true;
                child.explicit_height = true;
            }
        }
        self
    }

    /// Attach a raster image. Usually you'll want the [`crate::image`]
    /// free builder instead, which sets [`crate::Kind::Image`] for you; this
    /// method exists for cases where you've already constructed an El
    /// (e.g. through a stock widget) and want to swap in pixel art.
    pub fn image(mut self, image: impl Into<Image>) -> Self {
        self.image = Some(image.into());
        self
    }

    pub fn image_fit(mut self, fit: ImageFit) -> Self {
        self.image_fit = fit;
        self
    }

    pub fn image_tint(mut self, c: Color) -> Self {
        self.image_tint = Some(c);
        self
    }

    /// Attach an app-owned GPU texture source. Typically set via the
    /// [`crate::tree::surface`] builder (which also sets
    /// [`crate::Kind::Surface`]); reach for this method on a stock
    /// widget El whose Kind you want to keep.
    pub fn surface_source(mut self, source: crate::surface::SurfaceSource) -> Self {
        self.surface_source = Some(source);
        self
    }

    /// Attach a 3D scene specification. Typically set via the
    /// [`crate::tree::chart3d`] builder (which also sets
    /// [`crate::Kind::Scene3D`]).
    pub fn scene_source(mut self, scene: crate::scene::SceneSpec) -> Self {
        self.scene_source = Some(Box::new(scene));
        self
    }

    /// How a [`crate::Kind::Surface`] El composes with widgets below
    /// it. Default is [`crate::surface::SurfaceAlpha::Premultiplied`].
    pub fn surface_alpha(mut self, alpha: crate::surface::SurfaceAlpha) -> Self {
        self.surface_alpha = alpha;
        self
    }

    /// How a [`crate::Kind::Surface`] El's texture projects into its
    /// resolved rect. Defaults to [`crate::image::ImageFit::Fill`] —
    /// stretch to the rect — for parity with the pre-`surface_fit`
    /// behaviour. `Contain` / `Cover` / `None` mirror the modes on
    /// [`crate::El::image_fit`].
    pub fn surface_fit(mut self, fit: crate::image::ImageFit) -> Self {
        self.surface_fit = fit;
        self
    }

    /// Affine applied to the texture quad in destination space, around
    /// the centre of the post-[`Self::surface_fit`] rect. Defaults to
    /// identity. Use this for rotation, mirroring, source-dimension-
    /// independent zoom/pan, or any combination thereof. The El's
    /// auto-clip scissor still clamps the rendered content to the
    /// resolved rect.
    pub fn surface_transform(mut self, transform: crate::affine::Affine2) -> Self {
        self.surface_transform = transform;
        self
    }

    /// Attach a vector asset source. Typically set via the
    /// [`crate::tree::vector`] builder (which also sets
    /// [`crate::Kind::Vector`]); reach for this method on a stock
    /// widget El whose Kind you want to keep.
    pub fn vector_source(
        mut self,
        asset: impl Into<std::sync::Arc<crate::vector::VectorAsset>>,
    ) -> Self {
        self.vector_source = Some(asset.into());
        self
    }

    /// Select how a vector asset should render. The default is
    /// [`crate::vector::VectorRenderMode::Painted`], which preserves
    /// authored fills/strokes/gradients. Use [`Self::vector_mask`] when
    /// the asset is intended as one-colour coverage geometry.
    pub fn vector_render_mode(mut self, mode: crate::vector::VectorRenderMode) -> Self {
        self.vector_render_mode = mode;
        self
    }

    /// Treat this vector as coverage geometry and paint it with one
    /// colour. Backends can render this through their MSDF path.
    pub fn vector_mask(self, color: Color) -> Self {
        self.vector_render_mode(crate::vector::VectorRenderMode::Mask { color })
    }

    /// Preserve authored vector paint. This is the default for
    /// [`crate::tree::vector`].
    pub fn vector_painted(self) -> Self {
        self.vector_render_mode(crate::vector::VectorRenderMode::Painted)
    }

    /// Inside-out redraw deadline. While this El is visible (rect
    /// intersects the viewport), Damascene asks the host to drive the next
    /// frame within `deadline`. Aggregated across the tree via `min`,
    /// so the host gets a single signal regardless of how many widgets
    /// are asking. Use `Duration::ZERO` for "next frame ASAP";
    /// non-zero values pace the redraw loop below the display rate.
    ///
    /// Apps that pause / resume animation (e.g. GIF playback) just
    /// stop calling this method on the relevant El — Damascene re-runs
    /// the aggregation each frame, so the redraw scheduler quiets
    /// automatically when no visible widget is asking.
    pub fn redraw_within(mut self, deadline: std::time::Duration) -> Self {
        self.redraw_within = Some(deadline);
        self
    }

    /// Opt this node into the monospace face. Setting this flag also
    /// sets [`El::explicit_mono`] so a subsequent role modifier
    /// (`.caption()` / `.label()` / `.body()` / `.title()` /
    /// `.heading()` / `.display()`) won't silently reset `font_mono`
    /// when the role's default is non-mono. The natural reading order
    /// `text(s).mono().caption()` therefore renders in mono.
    pub fn mono(mut self) -> Self {
        self.font_mono = true;
        self.explicit_mono = true;
        self
    }

    /// Italic styling for a text run. Honoured by the
    /// [`crate::Kind::Inlines`] layout pass and (best-effort) on
    /// standalone text Els.
    pub fn italic(mut self) -> Self {
        self.text_italic = true;
        self
    }

    /// Inline-run background. Honoured when this El is a styled text
    /// leaf inside an [`crate::Kind::Inlines`] parent: the shaped span
    /// paints a solid quad behind its glyphs (per-line if the span
    /// wraps). Mirrors HTML's `<mark>` / inline `background`; the rect
    /// tracks the glyph extent rather than the El's layout box, so a
    /// wrapped highlight follows the prose. No effect on standalone
    /// text Els.
    pub fn background(mut self, color: Color) -> Self {
        self.text_bg = Some(color);
        self
    }

    /// Underline styling for a text run.
    pub fn underline(mut self) -> Self {
        self.text_underline = true;
        self
    }

    /// Strikethrough styling for a text run.
    pub fn strikethrough(mut self) -> Self {
        self.text_strikethrough = true;
        self
    }

    /// Markdown-flavoured inline-code styling. Currently `mono`-styled;
    /// a tinted background per the theme is a future addition. Authors
    /// who want raw mono without code chrome should use [`Self::mono`]
    /// instead.
    pub fn code(self) -> Self {
        self.text_role(TextRole::Code)
    }

    /// Mark this run as a link to `url`. Inside an
    /// [`crate::Kind::Inlines`] parent the run paints with a
    /// link-themed color; runs sharing the same URL group together for
    /// hit-test.
    pub fn link(mut self, url: impl Into<String>) -> Self {
        self.text_link = Some(url.into());
        self
    }

    pub fn math_expr(mut self, expr: impl Into<std::sync::Arc<crate::math::MathExpr>>) -> Self {
        self.math = Some(expr.into());
        self
    }

    pub fn math_display(mut self, display: crate::math::MathDisplay) -> Self {
        self.math_display = display;
        self
    }
}
