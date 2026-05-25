#![doc = include_str!("../README.md")]
//!
//! # Rendering smoke test
//!
//! Any `App` builds and renders headlessly through the bundle pipeline:
//!
//! ```
//! use aetna_core::prelude::*;
//!
//! struct Demo;
//!
//! impl App for Demo {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         card([
//!             card_header([card_title("Hello")]),
//!             card_content([text("rendered headlessly")]),
//!         ])
//!         .width(Size::Fixed(320.0))
//!     }
//! }
//!
//! let app = Demo;
//! let theme = app.theme();
//! let mut ui = app.build(&BuildCx::new(&theme));
//! let bundle = render_bundle(&mut ui, Rect::new(0.0, 0.0, 720.0, 400.0));
//! assert!(!bundle.svg.is_empty());
//! ```
//!
//! # Rendering pipeline
//!
//! Builders produce an [`El`] tree. Layout writes rects into [`UiState`].
//! The draw-op pass resolves visual facts into backend-neutral [`DrawOp`]
//! values. Backend runners turn those draw ops into GPU resources and
//! route pointer/keyboard input back into [`UiEvent`] values.
//!
//! The stock surface shader is `rounded_rect`; text, icons, custom
//! shaders, and backdrop-sampling materials all flow through the same
//! tree and event model.

pub mod affine;
pub mod anim;
pub mod bundle;
pub mod clipboard;
pub mod color;
pub mod cursor;
pub mod event;
pub mod focus;
pub mod hit_test;
pub mod icons;
pub mod image;
pub mod layout;
pub mod math;
pub mod metrics;
#[doc(hidden)]
pub mod paint;
// Back-compat re-exports of paint submodules at the crate root. These
// modules (draw_ops / ir / shader / surface) are referenced from ~85
// sites across this crate and downstream renderers / examples; the
// paint-pipeline regrouping shouldn't break those imports.
pub use paint::draw_ops;
pub use paint::ir;
pub use paint::shader;
pub use paint::surface;
pub mod prelude;
pub mod profile;
#[doc(hidden)]
pub mod runtime;
pub mod scene;
pub mod scroll;
pub mod selection;
pub mod state;
pub mod text;
pub mod theme;
// Back-compat re-exports of theme submodules at the crate root, since
// `tokens::`, `palette::`, and `style::` are referenced from ~130 call
// sites across this crate and downstream renderers / examples.
pub use theme::palette;
pub use theme::style;
pub use theme::tokens;
pub mod toast;
pub mod tooltip;
pub mod tree;
pub mod vector;
pub mod widgets;

// Prelude — for `use aetna_core::*;`.
pub use anim::{AnimProp, AnimValue, Animation, SpringConfig, Timing, TweenConfig};
pub use bundle::artifact::{
    Bundle, render_bundle, render_bundle_themed, render_bundle_with, render_bundle_with_theme,
    write_bundle,
};
pub use bundle::inspect::dump_tree;
pub use bundle::lint::{Finding, FindingKind, LintReport, lint};
pub use bundle::manifest::{draw_ops_text, shader_manifest};
pub use bundle::svg::svg_from_ops;
pub use clipboard::{delete_selection_event, paste_text_event};
pub use cursor::Cursor;
pub use draw_ops::{draw_ops, draw_ops_with_theme};
pub use event::{
    App, AppShader, BuildCx, FrameTrigger, HostDiagnostics, KeyChord, KeyModifiers, KeyPress,
    Pointer, PointerButton, PointerId, PointerKind, SurfaceColorInfo, SurfaceFormatInfo, UiEvent,
    UiEventKind, UiKey, UiTarget,
};
pub use focus::focus_order;
pub use hit_test::{hit_test, hit_test_target};
pub use icons::svg::{IconSource, IntoIconSource, SvgIcon, SvgIconPaintMode};
pub use icons::{IconStroke, all_icon_names, icon, icon_path, icon_strokes, icon_vector_asset};
pub use ir::{DrawOp, TextAnchor};
pub use layout::{LayoutCtx, LayoutFn, VirtualAnchorPolicy, VirtualItems, VirtualMode, layout};
pub use math::{
    MathAtom, MathDisplay, MathExpr, MathLayout, MathParseError, layout_math, parse_mathml,
    parse_mathml_with_display, parse_tex,
};
pub use metrics::{ComponentSize, MetricsRole, ThemeMetrics};
pub use shader::{ShaderBinding, ShaderHandle, StockShader, UniformBlock, UniformValue};
pub use state::{AnimationMode, UiState, WidgetState};
pub use style::StyleProfile;
pub use surface::{
    AppTexture, AppTextureBackend, AppTextureId, SurfaceAlpha, SurfaceFormat, SurfaceSource,
    next_app_texture_id,
};
pub use scene::{
    GeometryHandle, GeometryId, LineData, LineSegment, LinesHandle, MeshData, MeshHandle,
    MeshVertex, PointData, PointsHandle, ScenePoint,
};
// Atlas/glyph types are backend-implementer surface (consumed by
// `aetna-wgpu` / `aetna-vulkano` paint paths). App authors don't
// touch them, so hide from docs.rs while keeping them resolvable
// at the crate root for backend imports.
pub use palette::Palette;
pub use selection::{Selection, SelectionPoint, SelectionRange, selected_text};
#[doc(hidden)]
pub use text::atlas::{
    AtlasPage, AtlasRect, GlyphAtlas, GlyphKey, GlyphSlot, RunStyle, ShapedGlyph, ShapedRun,
};
pub use text::metrics::{
    MeasuredText, TextGeometry, TextHit, TextLayout, TextLine, caret_xy, caret_xy_with_family,
    hit_text, hit_text_with_family, layout_text, layout_text_with_family,
    layout_text_with_line_height_and_family, line_height, line_width, line_width_with_family,
    measure_text, selection_rects, selection_rects_with_family, wrap_lines, wrap_lines_with_family,
};
pub use theme::Theme;
pub use tree::{
    Align, Axis, Color, Corners, El, FontFamily, FontWeight, IconName, InteractionState, Justify,
    Kind, Rect, Sides, Size, Source, SurfaceRole, TextAlign, TextOverflow, TextRole, TextWrap,
    chart3d, column, divider, hard_break, math, math_block, math_inline, row, scroll, spacer,
    stack, surface, text_runs, vector, virtual_list, virtual_list_dyn,
};
pub use vector::{IconMaterial, VectorRenderMode};
// Vector path / mesh tessellation types are internal-tooling surface.
// `aetna_core::vector::*` keeps them reachable for tools that need
// raw mesh access; hide from docs.rs and the crate-root prelude so
// app authors aren't tempted to depend on them.
#[doc(hidden)]
pub use vector::{
    PathBuilder, VectorAsset, VectorColor, VectorFill, VectorFillRule, VectorGradient,
    VectorGradientStop, VectorLineCap, VectorLineJoin, VectorLinearGradient, VectorMesh,
    VectorMeshOptions, VectorMeshRun, VectorMeshVertex, VectorParseError, VectorPath,
    VectorRadialGradient, VectorSegment, VectorSpreadMethod, VectorStroke,
    append_vector_asset_mesh, parse_svg_asset, tessellate_vector_asset,
};

pub use widgets::accordion::{
    AccordionAction, accordion, accordion_content, accordion_item, accordion_item_key,
    accordion_separator, accordion_trigger, accordion_trigger_with_icon,
};
pub use widgets::alert::{alert, alert_description, alert_title};
pub use widgets::avatar::{DEFAULT_AVATAR_SIZE, avatar_fallback, avatar_image, avatar_initials};
pub use widgets::badge::badge;
pub use widgets::blockquote::blockquote;
pub use widgets::breadcrumb::{
    breadcrumb, breadcrumb_item, breadcrumb_link, breadcrumb_list, breadcrumb_page,
    breadcrumb_separator,
};
pub use widgets::button::{button, button_with_icon, icon_button};
pub use widgets::calendar::{CalendarAction, CalendarDay, calendar_day_key, calendar_month};
pub use widgets::card::{
    card, card_content, card_description, card_footer, card_header, card_title, titled_card,
};
pub use widgets::checkbox::checkbox;
pub use widgets::code_block::{code_block, code_block_chrome};
pub use widgets::command::{
    command_group, command_icon, command_item, command_label, command_row, command_shortcut,
};
pub use widgets::dialog::{
    dialog, dialog_content, dialog_description, dialog_footer, dialog_header, dialog_title,
};
pub use widgets::dropdown_menu::{
    dropdown_menu, dropdown_menu_content, dropdown_menu_content_with_density, dropdown_menu_group,
    dropdown_menu_icon, dropdown_menu_item, dropdown_menu_item_label,
    dropdown_menu_item_with_density, dropdown_menu_item_with_icon,
    dropdown_menu_item_with_icon_and_shortcut, dropdown_menu_item_with_shortcut,
    dropdown_menu_label, dropdown_menu_separator, dropdown_menu_shortcut,
    dropdown_menu_with_density,
};
pub use widgets::editor_tabs::{
    ActiveTabStyle, CloseVisibility, EditorTabsAction, EditorTabsConfig, editor_tab,
    editor_tab_add_key, editor_tab_close_key, editor_tab_select_key, editor_tabs, editor_tabs_with,
};
pub use widgets::form::{
    field_row, form, form_control, form_description, form_item, form_label, form_message,
    form_section,
};
pub use widgets::input_otp::input_otp;
pub use widgets::item::{
    item, item_actions, item_content, item_description, item_footer, item_group, item_header,
    item_media, item_media_icon, item_separator, item_title,
};
pub use widgets::list::{bullet_list, numbered_list, numbered_list_from, task_list};
pub use widgets::menubar::{
    MenubarAction, menubar, menubar_content, menubar_group, menubar_icon, menubar_item,
    menubar_item_label, menubar_item_with_icon, menubar_item_with_icon_and_shortcut,
    menubar_item_with_shortcut, menubar_label, menubar_menu, menubar_separator, menubar_shortcut,
    menubar_trigger, menubar_trigger_key,
};
pub use widgets::number_scrubber::{ScrubDrag, ScrubberOpts, number_scrubber};
pub use widgets::numeric_input::{NumericInputOpts, numeric_input};
pub use widgets::overlay::{modal, modal_panel, overlay, overlays, scrim};
pub use widgets::pagination::{
    pagination, pagination_content, pagination_ellipsis, pagination_item, pagination_link,
    pagination_next, pagination_previous,
};
pub use widgets::popover::{
    Anchor, MenuDensity, Side, TOUCH_MENU_ITEM_HEIGHT, anchor_rect, apply_menu_density,
    context_menu, context_menu_with_density, dropdown, menu_item, menu_item_with_density, popover,
    popover_panel,
};
pub use widgets::progress::progress;
pub use widgets::radio::{RadioAction, radio_group, radio_item, radio_option_key};
pub use widgets::select::{
    SelectAction, select_menu, select_menu_with_density, select_option_key, select_trigger,
};
pub use widgets::separator::{separator, vertical_separator};
pub use widgets::sheet::{
    SheetSide, sheet, sheet_content, sheet_description, sheet_footer, sheet_header, sheet_title,
};
pub use widgets::sidebar::{
    sidebar, sidebar_group, sidebar_group_label, sidebar_header, sidebar_menu, sidebar_menu_button,
    sidebar_menu_button_with_icon, sidebar_menu_item, sidebar_menu_label,
};
pub use widgets::skeleton::{skeleton, skeleton_circle};
pub use widgets::slider::{SliderAction, slider};
pub use widgets::switch::switch;
pub use widgets::table::{
    table, table_body, table_cell, table_head, table_head_el, table_header, table_row,
};
pub use widgets::tabs::{
    TabsAction, tab_option_key, tab_trigger, tab_trigger_content, tabs_list,
    tabs_list_from_triggers,
};
pub use widgets::text::{h1, h2, h3, mono, paragraph, text};
pub use widgets::text_area::text_area;
pub use widgets::text_input::{
    ClipboardKind, MaskMode, TextInputOpts, TextSelection, text_input, text_input_with,
};
pub use widgets::toggle::{
    ToggleAction, toggle, toggle_group, toggle_group_multi, toggle_item, toggle_option_key,
};
pub use widgets::toolbar::{toolbar, toolbar_description, toolbar_group, toolbar_title};
