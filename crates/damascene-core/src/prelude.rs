//! App and widget author prelude.
//!
//! Import this in ordinary Damascene applications:
//!
//! ```
//! use damascene_core::prelude::*;
//! ```
//!
//! The prelude intentionally avoids backend-implementer surface
//! (`runtime::RunnerCore`, `paint::*`, `text::atlas::*`, vector mesh
//! tessellation) and frame-internal hit-test/focus helpers. Reach for
//! those via their explicit modules when needed.

pub use crate::affine::Affine2;
pub use crate::anim::{AnimProp, AnimValue, Animation, SpringConfig, Timing, TweenConfig};
pub use crate::bundle::artifact::{
    Bundle, render_bundle, render_bundle_themed, render_bundle_with, render_bundle_with_theme,
    write_bundle,
};
pub use crate::bundle::inspect::dump_tree;
pub use crate::bundle::lint::{Finding, FindingKind, LintReport, lint};
pub use crate::bundle::manifest::{draw_ops_text, shader_manifest};
pub use crate::bundle::svg::svg_from_ops;
pub use crate::cursor::Cursor;
pub use crate::event::{
    App, AppShader, BuildCx, EventCx, FrameTrigger, HostDiagnostics, KeyChord, KeyModifiers,
    KeyPress, Pointer, PointerButton, PointerId, PointerKind, SurfaceColorInfo, SurfaceFormatInfo,
    UiEvent, UiEventKind, UiKey, UiTarget,
};
pub use crate::icons::svg::{IconSource, IntoIconSource, SvgIcon, SvgIconPaintMode};
pub use crate::icons::{all_icon_names, icon};
pub use crate::image::{DynamicRangeLimit, Image, ImageFit, PixelFormat};
pub use crate::ir::{DrawOp, TextAnchor};
pub use crate::layout::{LayoutCtx, LayoutFn, VirtualAnchorPolicy, VirtualItems, VirtualMode};
pub use crate::math::{
    MathAtom, MathDisplay, MathExpr, MathLayout, MathParseError, layout_math, parse_mathml,
    parse_mathml_with_display, parse_tex,
};
pub use crate::metrics::{ComponentSize, MetricsRole, ThemeMetrics};
pub use crate::palette::Palette;
pub use crate::shader::{ShaderBinding, ShaderHandle, StockShader, UniformBlock, UniformValue};
pub use crate::state::{AnimationMode, WidgetState};
pub use crate::style::StyleProfile;
pub use crate::surface::{
    AppTexture, AppTextureBackend, AppTextureId, SurfaceAlpha, SurfaceFormat, SurfaceSource,
};
pub use crate::text::metrics::{
    MeasuredText, TextHit, TextLayout, TextLine, caret_xy, caret_xy_with_family, hit_text,
    hit_text_with_family, layout_text, layout_text_with_family,
    layout_text_with_line_height_and_family, line_height, line_width, line_width_with_family,
    measure_text, selection_rects, selection_rects_with_family, wrap_lines, wrap_lines_with_family,
};
pub use crate::theme::Theme;
pub use crate::toast::{Toast, ToastLevel, ToastSpec};
pub use crate::tokens;
pub use crate::tree::{
    Align, ArrowNav, Axis, Color, Corners, El, FontFamily, FontWeight, HoverAlpha, IconName,
    InteractionState, Justify, Kind, PinPolicy, Rect, Sides, Size, Source, SurfaceRole, TextAlign,
    TextOverflow, TextRole, TextWrap, chart3d, column, divider, hard_break, image, math,
    math_block, math_inline, row, scroll, spacer, stack, surface, text_runs, vector, virtual_list,
    virtual_list_dyn,
};
pub use crate::vector::VectorRenderMode;
pub use crate::vector::{
    IconMaterial, PathBuilder, VectorAsset, VectorColor, VectorFill, VectorFillRule, VectorLineCap,
    VectorLineJoin, VectorPath, VectorSegment, VectorStroke,
};
pub use crate::widgets::accordion::{
    self, AccordionAction, accordion, accordion_content, accordion_item, accordion_item_key,
    accordion_separator, accordion_trigger, accordion_trigger_with_icon,
};
pub use crate::widgets::alert::{alert, alert_description, alert_title};
pub use crate::widgets::avatar::{
    DEFAULT_AVATAR_SIZE, avatar_fallback, avatar_image, avatar_initials,
};
pub use crate::widgets::badge::badge;
pub use crate::widgets::blockquote::blockquote;
pub use crate::widgets::breadcrumb::{
    breadcrumb, breadcrumb_item, breadcrumb_link, breadcrumb_list, breadcrumb_page,
    breadcrumb_separator,
};
pub use crate::widgets::button::{button, button_with_icon, icon_button};
pub use crate::widgets::calendar::{
    self, CalendarAction, CalendarDay, calendar_day_key, calendar_month,
};
pub use crate::widgets::card::{
    card, card_content, card_description, card_footer, card_header, card_title, titled_card,
};
pub use crate::widgets::checkbox::{self, checkbox};
pub use crate::widgets::code_block::{code_block, code_block_chrome};
pub use crate::widgets::command::{
    self, command_group, command_icon, command_item, command_label, command_row, command_shortcut,
};
pub use crate::widgets::dialog::{
    dialog, dialog_content, dialog_description, dialog_footer, dialog_header, dialog_title,
};
pub use crate::widgets::dropdown_menu::{
    self, dropdown_menu, dropdown_menu_content, dropdown_menu_content_with_density,
    dropdown_menu_group, dropdown_menu_icon, dropdown_menu_item, dropdown_menu_item_label,
    dropdown_menu_item_with_density, dropdown_menu_item_with_icon,
    dropdown_menu_item_with_icon_and_shortcut, dropdown_menu_item_with_shortcut,
    dropdown_menu_label, dropdown_menu_separator, dropdown_menu_shortcut,
    dropdown_menu_with_density,
};
pub use crate::widgets::editor_tabs::{
    self, ActiveTabStyle, CloseVisibility, EditorTabsAction, EditorTabsConfig, editor_tab,
    editor_tab_add_key, editor_tab_close_key, editor_tab_select_key, editor_tabs, editor_tabs_with,
};
pub use crate::widgets::form::{
    field_row, form, form_control, form_description, form_item, form_label, form_message,
    form_section,
};
pub use crate::widgets::input_otp::{self, input_otp};
pub use crate::widgets::item::{
    self, item, item_actions, item_content, item_description, item_footer, item_group, item_header,
    item_media, item_media_icon, item_separator, item_title,
};
pub use crate::widgets::list::{bullet_list, numbered_list, numbered_list_from, task_list};
pub use crate::widgets::menubar::{
    self, MenubarAction, menubar, menubar_content, menubar_group, menubar_icon, menubar_item,
    menubar_item_label, menubar_item_with_icon, menubar_item_with_icon_and_shortcut,
    menubar_item_with_shortcut, menubar_label, menubar_menu, menubar_separator, menubar_shortcut,
    menubar_trigger, menubar_trigger_key,
};
pub use crate::widgets::number_scrubber::{self, ScrubDrag, ScrubberOpts, number_scrubber};
pub use crate::widgets::numeric_input::{self, NumericInputOpts, numeric_input};
pub use crate::widgets::overlay::{modal, modal_panel, overlay, overlays, scrim};
pub use crate::widgets::page::page;
pub use crate::widgets::pagination::{
    self, pagination, pagination_content, pagination_ellipsis, pagination_item, pagination_link,
    pagination_next, pagination_previous,
};
pub use crate::widgets::popover::{
    Anchor, MenuDensity, Side, TOUCH_MENU_ITEM_HEIGHT, anchor_rect, apply_menu_density,
    context_menu, context_menu_with_density, dropdown, menu_item, menu_item_with_density, popover,
    popover_panel,
};
pub use crate::widgets::progress::{
    self, progress, progress_indeterminate, progress_indeterminate_with_color, progress_with_color,
};
pub use crate::widgets::radio::{self, RadioAction, radio_group, radio_item, radio_option_key};
pub use crate::widgets::resize_handle::{self, ResizeDrag, ResizeWeightsDrag, resize_handle};
pub use crate::widgets::select::{
    self, SelectAction, select_menu, select_menu_with_density, select_option_key, select_trigger,
};
pub use crate::widgets::separator::{separator, vertical_separator};
pub use crate::widgets::sheet::{
    self, SheetSide, sheet, sheet_content, sheet_description, sheet_footer, sheet_header,
    sheet_title,
};
pub use crate::widgets::sidebar::{
    self, sidebar, sidebar_group, sidebar_group_label, sidebar_header, sidebar_menu,
    sidebar_menu_button, sidebar_menu_button_with_icon, sidebar_menu_item, sidebar_menu_label,
};
pub use crate::widgets::skeleton::{self, skeleton, skeleton_circle};
pub use crate::widgets::slider::{self, SliderAction, slider, slider_with_color};
pub use crate::widgets::spinner::{self, spinner, spinner_with_color, spinner_with_track};
pub use crate::widgets::switch::{self, switch};
pub use crate::widgets::table::{
    self, table, table_body, table_cell, table_head, table_head_el, table_header, table_row,
};
pub use crate::widgets::tabs::{
    self, TabsAction, tab_option_key, tab_trigger, tab_trigger_content, tabs_list,
    tabs_list_from_triggers,
};
pub use crate::widgets::text::{h1, h2, h3, mono, paragraph, text};
pub use crate::widgets::text_area::{self, TextAreaOpts, text_area, text_area_with};
pub use crate::widgets::text_input::{
    self, ClipboardKind, MaskMode, TextInputOpts, TextSelection, text_input, text_input_with,
};
pub use crate::widgets::toggle::{
    self, ToggleAction, toggle, toggle_group, toggle_group_multi, toggle_item, toggle_option_key,
};
pub use crate::widgets::toolbar::{
    self, toolbar, toolbar_description, toolbar_group, toolbar_title,
};

pub use crate::selection::{Selection, SelectionPoint, SelectionRange, selected_text};
