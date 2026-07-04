//! Default values for [`El`].
//!
//! Keeping this separate from the field list makes it easier to review
//! default-policy changes without scanning the whole node surface.

use crate::image::ImageFit;
use crate::math::MathDisplay;
use crate::style::StyleProfile;

use super::geometry::{Corners, Rect, Sides};
use super::layout_types::{Align, Axis, Justify, Size};
use super::node::El;
use super::semantics::{Kind, Source, SurfaceRole};
use super::text_types::{FontFamily, FontWeight, TextAlign, TextOverflow, TextRole, TextWrap};

impl Default for El {
    fn default() -> Self {
        Self {
            kind: Kind::Group,
            style_profile: StyleProfile::TextOnly,
            key: None,
            block_pointer: false,
            hit_overflow: Sides::zero(),
            focusable: false,
            focus_ring_placement: Default::default(),
            always_show_focus_ring: false,
            selectable: false,
            consumes_touch_drag: false,
            selection_source: None,
            capture_keys: false,
            alpha_follows_focused_ancestor: false,
            blink_when_focused: false,
            state_follows_interactive_ancestor: false,
            no_hover: false,
            hover_alpha: None,
            source: Source::default(),
            allow_lint: Vec::new(),
            axis: Axis::Overlay,
            gap: 0.0,
            padding: Sides::zero(),
            align: Align::Stretch,
            justify: Justify::Start,
            width: Size::Hug,
            height: Size::Hug,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            component_size: None,
            metrics_role: None,
            explicit_width: false,
            explicit_height: false,
            explicit_padding: false,
            explicit_gap: false,
            explicit_radius: false,
            explicit_font_family: false,
            explicit_mono_font_family: false,
            explicit_mono: false,
            fill: None,
            dim_fill: None,
            stroke: None,
            stroke_width: 0.0,
            radius: Corners::ZERO,
            shadow: 0.0,
            surface_role: SurfaceRole::None,
            paint_overflow: Sides::zero(),
            clip: false,
            scrollable: false,
            pin_policy: crate::tree::PinPolicy::None,
            arrow_nav: None,
            tooltip: None,
            cursor: None,
            cursor_pressed: None,
            shader_override: None,
            layout_override: None,
            virtual_items: None,
            scrollbar: false,
            scrollbar_gutter: false,
            user_resizable: false,
            text: None,
            text_color: None,
            text_align: TextAlign::Start,
            text_wrap: TextWrap::NoWrap,
            text_overflow: TextOverflow::Clip,
            text_role: TextRole::Body,
            text_max_lines: None,
            font_size: crate::tokens::TEXT_SM.size,
            line_height: crate::tokens::TEXT_SM.line_height,
            font_family: FontFamily::default(),
            mono_font_family: FontFamily::JetBrainsMono,
            font_weight: FontWeight::Regular,
            font_mono: false,
            text_italic: false,
            text_bg: None,
            text_underline: false,
            text_strikethrough: false,
            text_tabular_numerals: false,
            text_link: None,
            math: None,
            math_display: MathDisplay::Inline,
            icon: None,
            icon_stroke_width: 2.0,
            image: None,
            image_tint: None,
            image_fit: ImageFit::Contain,
            image_range_limit: crate::image::DynamicRangeLimit::NoLimit,
            surface_source: None,
            surface_alpha: crate::surface::SurfaceAlpha::Premultiplied,
            surface_fit: ImageFit::Fill,
            surface_transform: crate::affine::Affine2::IDENTITY,
            scene_source: None,
            plot_source: None,
            vector_source: None,
            vector_render_mode: crate::vector::VectorRenderMode::Painted,
            children: Vec::new(),
            opacity: 1.0,
            translate: (0.0, 0.0),
            scale: 1.0,
            viewport: None,
            animate: None,
            redraw_within: None,
            computed_id: empty_id(),
            computed_rect: Rect::default(),
        }
    }
}

/// Shared empty `computed_id` for freshly built nodes — a refcount
/// bump instead of a per-node allocation (`Arc<str>` has no non-
/// allocating `new()`).
fn empty_id() -> std::sync::Arc<str> {
    use std::sync::OnceLock;
    static EMPTY: OnceLock<std::sync::Arc<str>> = OnceLock::new();
    EMPTY.get_or_init(|| std::sync::Arc::from("")).clone()
}
