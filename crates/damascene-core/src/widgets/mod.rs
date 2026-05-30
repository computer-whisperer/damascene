//! Stock widget vocabulary — reach for these before composing
//! `column` / `row` / `button` / `text` by hand.
//!
//! # Catalog
//!
//! **Surfaces & shells**
//! - [`card`] — boxed content surface; `card([card_header, card_content, card_footer])` and `titled_card("Title", [...])`
//! - [`sidebar`] — nav rail; `sidebar([sidebar_header, sidebar_group([sidebar_group_label, sidebar_menu([sidebar_menu_button(...)])])])`
//! - [`toolbar`] — page-chrome header row; `toolbar([toolbar_title, spacer(), toolbar_group([...])])`
//! - [`dialog`] — modal dialog; `dialog(key, [dialog_header, body, dialog_footer])`
//! - [`sheet`] — edge-pinned modal; `sheet(key, SheetSide::Right, [sheet_header, body])`
//! - [`popover`] — anchored floating panel; `dropdown` and `context_menu` compose over it
//! - [`overlay`] — `modal` / `modal_panel` / `scrim` primitives behind dialog/sheet
//!
//! **Object rows & lists**
//! - [`item`] — clickable resource row (recent file, repo, project, person, asset entry); `item([item_media_icon, item_content([item_title, item_description]), item_actions([...])])` inside `item_group([...])`
//! - [`list`] — plain `bullet_list` / `numbered_list` / `task_list` for prose-style enumerations
//! - [`table`] — structured tabular data; `table([table_header([table_row([table_head(...)])]), table_body([...])])`
//! - [`accordion`] — collapsible section; `accordion_item("group", "key", "Title", open, [...])` + `accordion::apply_event`
//!
//! **Navigation**
//! - [`tabs`] — segmented control / tabs; `tabs_list(key, &current, options)` + `tabs::apply_event`; `tabs_list_from_triggers([...])` for icon/badge tabs
//! - [`editor_tabs`] — closable, reorderable editor tabs (think VS Code)
//! - [`menubar`] — top-level app menus; `menubar([menubar_trigger(...)])` + `menubar_menu(...)` + `menubar::apply_event`
//! - [`breadcrumb`] — `breadcrumb_list([breadcrumb_link(...), breadcrumb_separator(), breadcrumb_page(...)])`
//! - [`pagination`] — `pagination_content([pagination_previous(), pagination_link(...), pagination_next()])`
//! - [`dropdown_menu`] — `dropdown_menu(key, trigger, [dropdown_menu_item_with_shortcut(...)])`; collapses per-row `[Edit][Delete]` button pairs
//! - [`command`] — palette / menu rows with icon + label + shortcut; `command_row(...)` / `command_item(...)`
//!
//! **Inputs & forms**
//! - [`calendar`] — controlled date grid; `calendar_month(key, "May 2026", days)` + `calendar::apply_event`
//! - [`text_input`] / [`text_area`] — controlled text editing; app owns `(value, Selection)` and calls `apply_event`; fixed-height text areas also drain caret scroll requests after accepted events
//! - [`numeric_input`] — number entry with stepper / formatting; `.stacked()` opt switches to the `<input type="number">`-style chevron column
//! - [`number_scrubber`] — drag-to-scrub numeric cell (Figma/Blender shape); `number_scrubber(value, key)` + `ScrubDrag` / `number_scrubber::apply_event`
//! - [`input_otp`] — segmented one-time-password input
//! - [`select`] — controlled dropdown; `select_trigger(key, label)` + `select_menu(key, options)` + `SelectAction`
//! - [`switch`] / [`checkbox`] — controlled bools with `apply_event`
//! - [`radio`] — `radio_group(...)` + `radio_item(...)` + `RadioAction`
//! - [`toggle`] — single or grouped toggle buttons (think bold/italic toolbar)
//! - [`slider`] — controlled value bar; `slider(...).key(k)` + `SliderAction` / `slider::apply_input`
//! - [`form`] — `form([...])` + `form_item([form_label, form_control, form_description, form_message])` + `field_row(label, control)` + `form_section(...)`
//!
//! **Feedback & status**
//! - [`alert`] — callouts; `alert([alert_title, alert_description]).warning() / .info() / .destructive()`
//! - [`badge`] — status pill; `badge("Online").success() / .warning() / .destructive() / .info() / .muted()`
//! - [`progress`] — non-interactive value bar; `progress(value, color)`, also `progress_indeterminate(...)`
//! - [`spinner`] — loading indicator
//! - [`skeleton`] — loading placeholder; `skeleton().width(...)` / `skeleton_circle(size)`
//!
//! **Identity & content**
//! - [`avatar`] — `avatar_fallback("Name")` / `avatar_image(img)`; default size [`avatar::DEFAULT_AVATAR_SIZE`]
//! - [`text`] — text leaves with role modifiers (`h1`, `h2`, `h3`, `paragraph`, `mono`, `.label()`, `.caption()`, `.muted()`, `.code()`)
//! - [`blockquote`] — quoted block
//! - [`code_block`] — fenced code with optional chrome
//! - [`button`] — `button(label)` / `button_with_icon(...)` / `icon_button(...)`; `.primary()` / `.secondary()` / `.ghost()` / `.destructive()`
//!
//! **Structural primitives**
//! - [`separator`] — `separator()` / `vertical_separator()` (1px line, content-aware)
//! - [`resize_handle`] — `resize_handle(Axis::Row).key(...)` + `resize_handle::apply_event_fixed` / `apply_event_weights` for draggable splitters
//!
//! # Symmetry invariant
//!
//! These modules are pure compositions of the public widget-kit surface
//! (`El` builders, style profiles, focus opt-in). They ship no privileged
//! internals: an app crate can fork any of them and produce an equivalent
//! widget against the same public API. The invariant — *stock widgets get
//! no APIs that user widgets don't* — is what makes the library a
//! substrate rather than a fixed component library; everything here is
//! its proof.

pub mod accordion;
pub mod alert;
pub mod avatar;
pub mod badge;
pub mod blockquote;
pub mod breadcrumb;
pub mod button;
pub mod calendar;
pub mod card;
pub mod checkbox;
pub mod code_block;
pub mod command;
pub mod dialog;
pub mod dropdown_menu;
pub mod editor_tabs;
pub mod form;
pub mod input_otp;
pub mod item;
pub mod list;
pub mod menubar;
pub mod number_scrubber;
pub mod numeric_input;
pub mod overlay;
pub mod pagination;
pub mod popover;
pub mod progress;
pub mod radio;
pub mod resize_handle;
pub mod select;
pub mod separator;
pub mod sheet;
pub mod sidebar;
pub mod skeleton;
pub mod slider;
pub mod spinner;
pub mod switch;
pub mod table;
pub mod tabs;
pub mod text;
pub mod text_area;
pub mod text_input;
pub mod toggle;
pub mod toolbar;
