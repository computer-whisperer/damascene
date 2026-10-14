//! "Meridian Slicer" — a reproduction of the target design in
//! `references/workbench-validation/slicer/reference.png` (1280×800).
//!
//! Sibling of `examples/slicer.rs`, which is the same product genre drawn
//! straight from Dark Modern. This one is a *match* exercise: the target
//! is a slate-and-cyan palette rather than VS Code's neutral grey and
//! `#0078D4`, so it exercises the extension path
//! [`damascene_workbench::theme::register_workbench_tokens`] documents —
//! start from the crate's slot mapping, re-register the workbench keys
//! with the app's own values, keep the metrics.
//!
//! Everything structural is stock: `button_group`, `toggle_item`,
//! `select_trigger`, `checkbox`, `field_row`, `icon_button`,
//! `menubar_trigger`, plus `chrome::{status_bar, pane_header}`.
//!
//! Run: `cargo run -p damascene-workbench --example slicer_match`

use std::sync::LazyLock;

use damascene_core::prelude::*;
use damascene_workbench::{chrome::*, theme as workbench, tokens as vs};

// ---------------------------------------------------------------------
// Palette — the target's slate ramp and cyan accent, mapped onto the
// same slots the workbench theme uses, then re-registered over the
// workbench key namespace so `chrome::status_bar` and friends follow.
// ---------------------------------------------------------------------

/// `titleBar.activeBackground` / `statusBar.background` — the two end bars.
const C_BAR: Color = Color::srgb_u8(30, 33, 40);
/// `activityBar.background` — the tool rail, a step below the bars.
const C_RAIL: Color = Color::srgb_u8(23, 26, 32);
/// The 3D canvas: darkest surface in the window.
const C_VIEWPORT: Color = Color::srgb_u8(15, 17, 22);
/// `sideBar.background` — the settings panel body.
const C_PANEL: Color = Color::srgb_u8(25, 28, 34);
/// The panel's own header/footer strips, one step up from its body.
const C_STRIP: Color = Color::srgb_u8(29, 32, 39);
/// `panel.border` — region separators.
const C_BORDER: Color = Color::srgb_u8(42, 47, 56);
/// The quieter rule under a settings section header.
const C_RULE: Color = Color::srgb_u8(35, 39, 47);
/// `input.background` — every field trough.
const C_INPUT: Color = Color::srgb_u8(16, 19, 24);
/// `input.border`.
const C_INPUT_BORDER: Color = Color::srgb_u8(58, 65, 77);
/// `focusBorder` / `button.background` — the one accent in the design.
const C_ACCENT: Color = Color::srgb_u8(75, 184, 216);
/// `list.activeSelectionBackground` — the selected-segment wash.
const C_ACCENT_DIM: Color = Color::srgb_u8(30, 51, 62);
/// `button.foreground` — dark text on the cyan fill.
const C_ON_ACCENT: Color = Color::srgb_u8(10, 24, 30);
/// The "modified" amber.
const C_AMBER: Color = Color::srgb_u8(217, 151, 74);
/// The "ready" green.
const C_GREEN: Color = Color::srgb_u8(78, 176, 112);

const C_FG: Color = Color::srgb_u8(231, 235, 241);
const C_FG_MUTED: Color = Color::srgb_u8(140, 148, 161);
const C_FG_DIM: Color = Color::srgb_u8(92, 100, 114);
const C_FG_FAINT: Color = Color::srgb_u8(71, 78, 89);

/// The workbench slot mapping with the target's values substituted.
///
/// `register_workbench_tokens` is called *after* the substitution so the
/// crate's own chrome (`status_bar`, `pane_header`) repaints from these
/// values rather than from Dark Modern's — extra-token registration
/// keeps the last write for a name.
fn palette() -> Palette {
    let p = Palette {
        background: C_VIEWPORT,
        foreground: C_FG,

        card: C_PANEL,
        card_foreground: C_FG,

        popover: C_STRIP,
        popover_foreground: C_FG,

        primary: C_ACCENT,
        primary_foreground: C_ON_ACCENT,

        secondary: C_STRIP,
        secondary_foreground: C_FG_MUTED,

        // `SurfaceRole::Input` derives its default trough from
        // `muted.darken(0.08)` and its stroke from `input` at 75% alpha
        // (`Theme::apply_role_material`). Authored `.fill()`/`.stroke()`
        // now win over those defaults, but routing through the slots
        // keeps every stock `select_trigger`/`text_input` themed at
        // once — pre-compensated here so the defaults land on
        // `C_INPUT` / `C_INPUT_BORDER` over the panel.
        muted: Color::srgb_u8(19, 22, 28),
        muted_foreground: C_FG_DIM,

        accent: C_ACCENT_DIM,
        accent_foreground: C_ACCENT,

        border: C_BORDER,
        input: Color::srgb_u8(66, 74, 88),
        ring: C_ACCENT,

        success: C_GREEN,
        warning: C_AMBER,
        info: C_ACCENT,
        link_foreground: C_ACCENT,

        selection_bg: C_ACCENT_DIM,

        ..workbench::dark_modern_palette()
    };

    workbench::register_workbench_tokens(p)
        .with_token("titleBar.activeBackground", C_BAR)
        .with_token("titleBar.activeForeground", C_FG)
        .with_token("titleBar.border", C_BORDER)
        .with_token("statusBar.background", C_BAR)
        .with_token("statusBar.foreground", C_FG_MUTED)
        .with_token("statusBar.border", C_BORDER)
        .with_token("activityBar.background", C_RAIL)
        .with_token("activityBar.border", C_BORDER)
        .with_token("activityBar.foreground", C_ACCENT)
        .with_token("activityBar.inactiveForeground", C_FG_DIM)
        .with_token("sideBar.background", C_PANEL)
        .with_token("sideBar.border", C_BORDER)
        .with_token("sideBarSectionHeader.background", C_PANEL)
        .with_token("sideBarSectionHeader.foreground", C_FG_MUTED)
        .with_token("sideBarSectionHeader.border", C_RULE)
        .with_token("panel.border", C_BORDER)
        .with_token("editor.background", C_VIEWPORT)
        .with_token("input.background", C_INPUT)
        .with_token("input.border", C_INPUT_BORDER)
        .with_token("dropdown.background", C_INPUT)
        .with_token("dropdown.border", C_INPUT_BORDER)
        .with_token("checkbox.background", C_INPUT)
        .with_token("checkbox.border", C_INPUT_BORDER)
        .with_token("focusBorder", C_ACCENT)
        .with_token("button.background", C_ACCENT)
        .with_token("button.foreground", C_ON_ACCENT)
        .with_token("list.activeSelectionBackground", C_ACCENT_DIM)
        .with_token("list.inactiveSelectionBackground", C_ACCENT_DIM)
}

// ---------------------------------------------------------------------
// App glyphs.
//
// The built-in vocabulary (`all_icon_names()`, 57 entries) now covers the
// transform gizmos — `move`, `rotate-cw`, `scaling`, `flip-horizontal`,
// `ruler`, `camera` — so the tool rail draws built-ins. What remains here
// is genuinely product-specific: the brand mark, undo/redo, a print head,
// a slice action, a filament spool, a support lattice, the mesh and axis
// placeholders. These go through the documented
// `SvgIcon::parse_current_color` path, so they tint from `text_color`
// exactly like the built-ins.
// ---------------------------------------------------------------------

const GLYPH_HEAD: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">"##;

macro_rules! glyph {
    ($name:ident, $body:expr) => {
        static $name: LazyLock<SvgIcon> = LazyLock::new(|| {
            SvgIcon::parse_current_color(&format!("{GLYPH_HEAD}{}</svg>", $body))
                .expect("app glyph parses")
        });
    };
}

glyph!(
    MARK,
    r##"<circle cx="12" cy="12" r="9"/><path d="M3 12h18"/><path d="M12 3a14 14 0 0 1 0 18a14 14 0 0 1 0-18z"/>"##
);
glyph!(UNDO, r##"<path d="M4 8h11a5 5 0 0 1 0 10H8"/><path d="m8 4-4 4 4 4"/>"##);
glyph!(
    REDO,
    r##"<path d="M20 8H9a5 5 0 0 0 0 10h7"/><path d="m16 4 4 4-4 4"/>"##
);
glyph!(
    PRINTER,
    r##"<path d="M7 9V3h10v6"/><path d="M5 9h14a2 2 0 0 1 2 2v5h-4"/><path d="M7 16H3v-5a2 2 0 0 1 2-2"/><path d="M7 14h10v7H7z"/>"##
);
glyph!(
    SLICE,
    r##"<path d="m12 3 9 5-9 5-9-5z"/><path d="m3 13 9 5 9-5"/>"##
);
glyph!(
    SUN,
    r##"<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M2 12h2M20 12h2M5 5l1.5 1.5M17.5 17.5 19 19M19 5l-1.5 1.5M6.5 17.5 5 19"/>"##
);
glyph!(
    GRID,
    r##"<rect x="3" y="3" width="18" height="18" rx="1"/><path d="M3 9h18M3 15h18M9 3v18M15 3v18"/>"##
);
glyph!(
    SUPPORTS,
    r##"<path d="M4 4v16M12 4v16M20 4v16"/><path d="M2 21h20M2 3h20"/>"##
);
glyph!(
    CLOCK,
    r##"<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3.5 2"/>"##
);
glyph!(
    SPOOL,
    r##"<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="4"/><path d="M12 3v5M12 16v5"/>"##
);
glyph!(
    CUBE,
    r##"<path d="m12 2.5 9 5v9l-9 5-9-5v-9z"/><path d="m12 12.5 9-5M12 12.5v9M12 12.5l-9-5"/>"##
);
glyph!(
    AXES,
    r##"<path d="M12 17V3"/><path d="M12 17 2 22"/><path d="m12 17 10 5"/>"##
);

// ---------------------------------------------------------------------
// Metrics measured off the reference.
// ---------------------------------------------------------------------

const TITLE_H: f32 = 36.0;
const RAIL_W: f32 = 48.0;
const PANEL_W: f32 = 300.0;
const PANEL_PAD: f32 = 11.0;
const CTRL_W: f32 = 132.0;
const CTRL_H: f32 = 20.0;
const ROW_GAP: f32 = 9.0;
const SECTION_HEADER_H: f32 = 33.0;

const VIEW_KEY: &str = "view";
const OVERLAY_KEY: &str = "overlay";
const TOOL_KEY: &str = "tool";
const PRESET_KEY: &str = "preset";

struct Slicer {
    view: String,
    overlay: String,
    tool: String,
    preset: String,
}

impl Slicer {
    fn new() -> Self {
        Self {
            view: "iso".into(),
            overlay: "grid".into(),
            tool: "move".into(),
            preset: "20".into(),
        }
    }

    // -----------------------------------------------------------------
    // Shared recipes.
    // -----------------------------------------------------------------

    /// A read-only value trough: right-aligned mono value with a dim
    /// unit suffix.
    ///
    /// `numeric_input` is the stock widget for this slot, but it always
    /// paints steppers, always left-aligns, and has no unit slot — see
    /// the example's gap notes. Deliberately *not* on
    /// `SurfaceRole::Input`: that role overrides the author's fill and
    /// stroke, which would take the focused-Density variant with it.
    fn value_field(value: &str, unit: &str) -> El {
        row([
            text(value)
                .body()
                .mono()
                .text_color(C_FG)
                .tabular_numerals()
                .width(Size::Fill(1.0))
                .end_text(),
            text(unit).caption().mono().text_color(C_FG_FAINT),
        ])
        .gap(tokens::SPACE_1)
        .fill(C_INPUT)
        .stroke(C_INPUT_BORDER)
        .radius(vs::RADIUS)
        .padding(Sides::x(tokens::SPACE_2))
        .width(Size::Fixed(CTRL_W))
        .height(Size::Fixed(CTRL_H))
        .align(Align::Center)
    }

    /// A select sized for the panel's label/control split. The stock
    /// trigger is `Fill(1.0)`, which would eat `field_row`'s spacer.
    fn panel_select(key: &str, label: &str, mono: bool) -> El {
        let t = select_trigger(key.to_string(), label.to_string())
            .fill(vs::DROPDOWN_BG)
            .stroke(vs::DROPDOWN_BORDER)
            .text_color(C_FG)
            .padding(Sides::x(tokens::SPACE_2))
            .width(Size::Fixed(CTRL_W))
            .height(Size::Fixed(CTRL_H));
        if mono { t.mono() } else { t }
    }

    /// A bordered segmented control: stock `button_group` in a trough.
    ///
    /// `button_group` is the public joiner that takes pre-built
    /// children — needed here because two of the three groups are not
    /// `toggle_item`s (the overlay pair is `button_with_icon`, see
    /// below) while `toggle_group` builds its own items from
    /// `(value, label)` pairs. It brings the corner collapse, the
    /// flush-neighbour hygiene, and the seams; `.stroke(...)` on the
    /// group paints frame *and* seams in this target's `input.border`.
    ///
    /// What is left is match-exercise calibration: the trough fill, the
    /// per-item height, and the `clip()` + square segments that make the
    /// trough own the outer silhouette (`RADIUS_SM` here rather than the
    /// segments' own `RADIUS_MD`).
    fn segmented(items: Vec<El>, height: f32) -> El {
        button_group(items.into_iter().map(|it| {
            it.height(Size::Fixed(height))
                .radius(0.0)
                .font_weight(FontWeight::Medium)
        }))
        .arrow_nav(ArrowNav::Horizontal)
        .fill(C_INPUT)
        .stroke(C_INPUT_BORDER)
        .radius(tokens::RADIUS_SM)
        .clip()
        .height(Size::Fixed(height))
    }

    /// The count disc on a section header. `chrome::chip` is the stock
    /// slot, but it is a solid fill — the target's is an outlined disc.
    fn count(n: &str) -> El {
        row([text(n).caption().text_color(C_FG_DIM)])
            .stroke(C_INPUT_BORDER)
            .radius(8.0)
            .width(Size::Fixed(16.0))
            .height(Size::Fixed(16.0))
            .align(Align::Center)
            .justify(Justify::Center)
    }

    /// An outlined status tag — `chrome::chip` in an outline flavor.
    fn tag(label: &str, tint: Color) -> El {
        row([text(label).caption().mono().text_color(tint)])
            .stroke(tint.with_alpha_u8(110))
            .radius(tokens::RADIUS_PILL)
            .padding(Sides::x(tokens::SPACE_2))
            .height(Size::Fixed(17.0))
            .width(Size::Hug)
            .align(Align::Center)
            .justify(Justify::Center)
    }

    /// A status-bar readout pill: leading glyph, then mono text.
    /// `chrome::chip` takes a `String` only, so the glyph slot is local.
    fn pill(g: &'static LazyLock<SvgIcon>, tint: Color, body: Vec<El>) -> El {
        let mut children = vec![icon(&**g).icon_size(12.0).text_color(tint)];
        children.extend(body);
        row(children)
            .gap(tokens::SPACE_1)
            .stroke(C_BORDER)
            .radius(tokens::RADIUS_PILL)
            .padding(Sides::x(tokens::SPACE_2))
            .height(Size::Fixed(17.0))
            .width(Size::Hug)
            .align(Align::Center)
    }

    fn dot(color: Color, size: f32) -> El {
        divider()
            .width(Size::Fixed(size))
            .height(Size::Fixed(size))
            .radius(tokens::RADIUS_PILL)
            .fill(color)
    }

    fn bar_separator() -> El {
        divider()
            .width(Size::Fixed(1.0))
            .height(Size::Fixed(12.0))
            .fill(C_BORDER)
    }

    // -----------------------------------------------------------------
    // Title / menu bar.
    // -----------------------------------------------------------------

    fn title_bar(&self) -> El {
        let menu = |value: &str, label: &str| {
            menubar_trigger("menu", value, label, false)
                .height(Size::Fixed(22.0))
                .font_size(12.0)
                .text_color(C_FG_MUTED)
        };
        let chrome_button = |key: &str, g: &'static LazyLock<SvgIcon>| {
            icon_button(&**g)
                .key(key.to_string())
                .ghost()
                .icon_size(tokens::ICON_SM)
                .text_color(C_FG_DIM)
                .width(Size::Fixed(26.0))
                .height(Size::Fixed(24.0))
        };

        row([
            row([
                icon(&*MARK).icon_size(tokens::ICON_SM).text_color(C_ACCENT),
                text("Meridian Slicer")
                    .body()
                    .font_size(13.0)
                    .font_weight(FontWeight::Semibold)
                    .text_color(C_FG),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
            row([
                menu("file", "File"),
                menu("edit", "Edit"),
                menu("view", "View"),
                menu("help", "Help"),
            ])
            .gap(tokens::SPACE_1)
            .align(Align::Center),
            spacer(),
            row([
                text("bracket_v4_final.stl")
                    .body()
                    .font_size(12.0)
                    .font_weight(FontWeight::Medium)
                    .text_color(C_FG_MUTED),
                text("—").body().font_size(12.0).text_color(C_FG_FAINT),
                text("Prusa MK4S  ·  0.4 mm nozzle")
                    .body()
                    .font_size(12.0)
                    .text_color(C_FG_DIM),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
            spacer(),
            row([
                chrome_button("undo", &UNDO),
                chrome_button("redo", &REDO),
                Self::bar_separator(),
                chrome_button("print", &PRINTER),
            ])
            .gap(tokens::SPACE_1)
            .align(Align::Center),
            // `button_with_icon` rather than `button`: its label is a
            // *child*, so the ⌘R hint can be appended as a third child.
            // A `button()`'s label lives on the node itself, and an
            // appended child paints on top of it.
            button_with_icon(&*SLICE, "Slice")
                .key("slice")
                .primary()
                .size(ComponentSize::Xs)
                .height(Size::Fixed(24.0))
                .padding(Sides::x(tokens::SPACE_2))
                .gap(tokens::SPACE_2)
                .child(
                    text("⌘R")
                        .caption()
                        .mono()
                        .text_color(C_ON_ACCENT)
                        .opacity(0.7),
                ),
        ])
        .gap(tokens::SPACE_3)
        .fill(vs::TITLE_BAR_ACTIVE_BG)
        .border_b()
        .border_color(vs::TITLE_BAR_BORDER)
        .padding(Sides::x(tokens::SPACE_3))
        .height(Size::Fixed(TITLE_H))
        .width(Size::Fill(1.0))
        .align(Align::Center)
    }

    // -----------------------------------------------------------------
    // Tool rail — VS Code's activity bar as a transform-gizmo rail.
    // -----------------------------------------------------------------

    fn tool_rail(&self) -> El {
        // `IconSource` rather than one concrete type, because the gizmos
        // are built-ins and the appearance toggle is an app glyph.
        let tool = |id: &'static str, g: IconSource, label: &str| {
            let active = self.tool == id;
            let b = icon_button(g)
                .key(format!("{TOOL_KEY}:{id}"))
                .tooltip(label.to_string())
                .ghost()
                .icon_size(tokens::ICON_MD)
                .width(Size::Fixed(RAIL_W - 1.0))
                .height(Size::Fixed(42.0))
                .radius(0.0);
            if active {
                b.fill(C_ACCENT_DIM.with_alpha_u8(150))
                    .text_color(vs::ACTIVITY_BAR_FG)
                    .border_l()
                    .border_widths(Sides {
                        left: 2.0,
                        right: 0.0,
                        top: 0.0,
                        bottom: 0.0,
                    })
                    .border_color(C_ACCENT)
            } else {
                b.text_color(vs::ACTIVITY_BAR_INACTIVE_FG)
            }
        };

        column([
            tool("move", IconName::Move.into_icon_source(), "Move"),
            tool("rotate", IconName::RotateCw.into_icon_source(), "Rotate"),
            tool("scale", IconName::Scaling.into_icon_source(), "Scale"),
            tool(
                "mirror",
                IconName::FlipHorizontal.into_icon_source(),
                "Mirror",
            ),
            tool("measure", IconName::Ruler.into_icon_source(), "Measure"),
            tool("camera", IconName::Camera.into_icon_source(), "Camera view"),
            spacer(),
            tool("theme", (&*SUN).into_icon_source(), "Appearance"),
        ])
        .gap(tokens::SPACE_0)
        .padding(Sides::y(tokens::SPACE_1))
        .fill(vs::ACTIVITY_BAR_BG)
        .border_r()
        .border_color(vs::ACTIVITY_BAR_BORDER)
        .width(Size::Fixed(RAIL_W))
        .height(Size::Fill(1.0))
        .align(Align::Center)
    }

    // -----------------------------------------------------------------
    // Viewport.
    // -----------------------------------------------------------------

    fn viewport(&self) -> El {
        let view_item = |id: &str, label: &str| {
            toggle_item(VIEW_KEY, id, label, self.view == id).padding(Sides::x(tokens::SPACE_3))
        };
        // `toggle_item` has no leading-icon flavor (core ships
        // `button_with_icon` and `sidebar_menu_button_with_icon` but no
        // `toggle_item_with_icon`), so these two segments are a
        // `button_with_icon` carrying the same routed key by hand.
        let overlay_item = |id: &str, label: &str, g: &'static LazyLock<SvgIcon>| {
            let on = self.overlay == id;
            let b = button_with_icon(&**g, label)
                .key(toggle_option_key(OVERLAY_KEY, &id))
                .size(ComponentSize::Xs)
                .padding(Sides::x(tokens::SPACE_2))
                .gap(tokens::SPACE_1);
            if on { b.current() } else { b.ghost() }
        };

        column([
            row([
                Self::segmented(
                    vec![
                        view_item("iso", "ISO"),
                        view_item("top", "TOP"),
                        view_item("front", "FRONT"),
                        view_item("right", "RIGHT"),
                    ],
                    22.0,
                ),
                Self::segmented(
                    vec![
                        overlay_item("grid", "Grid", &GRID),
                        overlay_item("supports", "Supports", &SUPPORTS),
                    ],
                    22.0,
                ),
            ])
            .gap(tokens::SPACE_3)
            .width(Size::Fill(1.0))
            .align(Align::Center),
            spacer(),
            // Content-level placeholder: the real mesh, build plate and
            // grid are the app's 3D scene, not chrome.
            column([
                icon(&*CUBE).icon_size(38.0).text_color(C_FG_FAINT),
                text("bracket_v4_final.stl")
                    .title()
                    .mono()
                    .font_size(15.0)
                    .text_color(Color::srgb_u8(74, 82, 97)),
                text("142,368 triangles    ·    84.2 × 61.0 × 27.5 mm    ·    1 instance")
                    .caption()
                    .mono()
                    .font_size(10.5)
                    .text_color(Color::srgb_u8(54, 61, 73)),
            ])
            .gap(tokens::SPACE_3)
            .width(Size::Fill(1.0))
            .align(Align::Center),
            spacer(),
            row([
                text("250 × 210 × 220 mm build volume")
                    .caption()
                    .mono()
                    .text_color(Color::srgb_u8(58, 65, 77)),
                spacer(),
                icon(&*AXES).icon_size(46.0).text_color(C_FG_FAINT),
            ])
            .width(Size::Fill(1.0))
            .align(Align::End),
        ])
        .gap(tokens::SPACE_2)
        .padding(tokens::SPACE_3)
        .fill(vs::EDITOR_BG)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    // -----------------------------------------------------------------
    // Settings panel.
    // -----------------------------------------------------------------

    /// `chrome::pane_header` at the target's height, over a padded
    /// column of `field_row`s.
    fn section<I, E>(title: &str, trailing: Vec<El>, rows: I) -> El
    where
        I: IntoIterator<Item = E>,
        E: Into<El>,
    {
        column([
            // `Align::Center` inside `pane_header` puts the title 5px
            // above where the target sets it; asymmetric padding is the
            // only lever, since the title node is not reachable.
            pane_header(title.to_string(), trailing)
                .padding(Sides {
                    left: PANEL_PAD,
                    right: PANEL_PAD,
                    top: 10.0,
                    bottom: 0.0,
                })
                .height(Size::Fixed(SECTION_HEADER_H)),
            column(rows)
                .gap(ROW_GAP)
                .padding(Sides::xy(PANEL_PAD, ROW_GAP))
                .text_color(C_FG_MUTED)
                .width(Size::Fill(1.0)),
        ])
        .width(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    fn settings_panel(&self) -> El {
        let preset = |v: &str| {
            toggle_item(PRESET_KEY, v, v, self.preset == v)
                .mono()
                .padding(Sides::x(0.0))
                .width(Size::Fill(1.0))
        };

        column([
            // Profile strip.
            row([
                text("PROFILE")
                    .caption()
                    .letter_spacing(1.0)
                    .text_color(C_FG_DIM),
                Self::panel_select("profile", "0.20 mm SPEED — PLA", false)
                    .width(Size::Fill(1.0)),
            ])
            .gap(tokens::SPACE_2)
            .fill(C_STRIP)
            .border_b()
            .border_color(C_BORDER)
            .padding(Sides::x(PANEL_PAD))
            .height(Size::Fixed(38.0))
            .width(Size::Fill(1.0))
            .align(Align::Center),
            column([
                Self::section(
                    "QUALITY",
                    vec![Self::count("4")],
                    [
                        field_row(
                            "Layer height",
                            Self::panel_select("q:layer", "0.20 mm", true),
                        ),
                        field_row("First layer height", Self::value_field("0.25", "mm")),
                        field_row("Wall count", Self::value_field("3", "walls")),
                        field_row("Top / bottom layers", Self::value_field("5 / 4", "×")),
                    ],
                ),
                Self::section(
                    "INFILL",
                    vec![Self::tag("modified", C_AMBER)],
                    [
                        // `field_row` takes a plain `String` label, so the
                        // trailing modified-dot needs the row spelled out.
                        row([
                            text("Density").label(),
                            Self::dot(C_AMBER, 5.0),
                            spacer(),
                            Self::value_field("20", "%")
                                .stroke(C_ACCENT)
                                .radius(tokens::RADIUS_SM),
                        ])
                        .gap(tokens::SPACE_2)
                        .width(Size::Fill(1.0))
                        .align(Align::Center),
                        field_row(
                            "Preset",
                            Self::segmented(
                                vec![
                                    preset("0"),
                                    preset("10"),
                                    preset("20"),
                                    preset("40"),
                                    preset("100"),
                                ],
                                CTRL_H,
                            )
                            .width(Size::Fixed(CTRL_W)),
                        ),
                        field_row("Pattern", Self::panel_select("i:pattern", "Gyroid", false)),
                        field_row("Wall overlap", Self::value_field("10", "%")),
                    ],
                ),
                Self::section(
                    "MATERIAL",
                    vec![Self::count("4")],
                    [
                        field_row("Material", Self::panel_select("m:mat", "PLA", false)),
                        field_row(
                            "Filament",
                            Self::panel_select("m:fil", "Prusament PLA G…", false),
                        ),
                        field_row("Nozzle temperature", Self::value_field("215", "°C")),
                        field_row("Bed temperature", Self::value_field("60", "°C")),
                    ],
                ),
                Self::section(
                    "SUPPORTS",
                    vec![Self::count("3")],
                    [
                        field_row("Generate supports", checkbox("s:on", true)),
                        field_row("Overhang angle", Self::value_field("55", "°")),
                        field_row(
                            "Placement",
                            Self::panel_select("s:place", "Touching build pl…", false),
                        ),
                        field_row("Z contact distance", Self::value_field("0.20", "mm")),
                    ],
                ),
                spacer(),
            ])
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Stretch)
            .scrollable(),
            // Footer.
            row([
                Self::dot(C_AMBER, 5.0),
                text("1 setting differs from profile")
                    .caption()
                    .text_color(C_FG_MUTED),
                spacer(),
                text("Reset").caption().text_color(C_ACCENT),
            ])
            .gap(tokens::SPACE_2)
            .fill(C_STRIP)
            .border_t()
            .border_color(C_BORDER)
            .padding(Sides::x(PANEL_PAD))
            .height(Size::Fixed(27.0))
            .width(Size::Fill(1.0))
            .align(Align::Center),
        ])
        .fill(vs::SIDE_BAR_BG)
        .border_l()
        .border_color(vs::SIDE_BAR_BORDER)
        .width(Size::Fixed(PANEL_W))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
    }
}

impl App for Slicer {
    fn build(&self, _cx: &BuildCx) -> El {
        // A bare column root, not `page()`: a workbench shell is
        // full-bleed by definition.
        column([
            self.title_bar(),
            row([self.tool_rail(), self.viewport(), self.settings_panel()])
                .height(Size::Fill(1.0))
                .width(Size::Fill(1.0))
                .align(Align::Stretch),
            status_bar(
                [
                    row([
                        Slicer::dot(C_GREEN, 6.0),
                        text("Ready").caption().text_color(C_FG_MUTED),
                    ])
                    .gap(tokens::SPACE_1)
                    .align(Align::Center),
                    Slicer::bar_separator(),
                    row([
                        icon(IconName::FileText)
                            .icon_size(12.0)
                            .text_color(C_FG_DIM),
                        text("bracket_v4_final.stl")
                            .caption()
                            .mono()
                            .text_color(C_FG_MUTED),
                        text("142,368 tris").caption().mono().text_color(C_FG_DIM),
                    ])
                    .gap(tokens::SPACE_2)
                    .align(Align::Center),
                    Slicer::bar_separator(),
                    text("Sliced 12 s ago").caption().text_color(C_FG_MUTED),
                ],
                [
                    text("0.20 mm  ·  20% gyroid")
                        .caption()
                        .mono()
                        .text_color(C_FG_DIM),
                    Slicer::pill(
                        &CLOCK,
                        C_ACCENT,
                        vec![
                            text("4 h 12").caption().mono().text_color(C_FG),
                            text("m").caption().mono().text_color(C_FG_FAINT),
                        ],
                    ),
                    Slicer::pill(
                        &SPOOL,
                        C_AMBER,
                        vec![
                            text("38.4 g").caption().mono().text_color(C_FG),
                            text("/").caption().mono().text_color(C_FG_FAINT),
                            text("12.86 m").caption().mono().text_color(C_FG_DIM),
                        ],
                    ),
                    Slicer::pill(
                        &PRINTER,
                        C_FG_DIM,
                        vec![text("MK4S").caption().mono().text_color(C_FG_MUTED)],
                    ),
                ],
            ),
        ])
        .align(Align::Stretch)
        .fill(vs::EDITOR_BG)
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        let id = |s: &str| Some(s.to_string());
        if toggle::apply_event_single(&mut self.view, &event, VIEW_KEY, id) {
            return;
        }
        if toggle::apply_event_single(&mut self.overlay, &event, OVERLAY_KEY, id) {
            return;
        }
        if toggle::apply_event_single(&mut self.preset, &event, PRESET_KEY, id) {
            return;
        }
        for t in ["move", "rotate", "scale", "mirror", "measure", "camera"] {
            if event.is_click_or_activate(&format!("{TOOL_KEY}:{t}")) {
                self.tool = t.to_string();
                return;
            }
        }
    }

    fn theme(&self) -> Theme {
        // The target is denser than the workbench profile's own 13/14
        // scale: its field labels measure ~11px and its values ~10.5px,
        // where `theme::TYPE_SCALE` puts a `.label()` at 13px. One knob
        // moves the whole ladder — the few nodes that want the old size
        // (app title, menus) carry a hand-picked `.font_size()`, which
        // the scale leaves alone.
        workbench::theme()
            .with_type_scale(11.0 / 14.0)
            .with_palette(palette())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 1280.0, 800.0);
    damascene_winit_wgpu::run("Meridian Slicer", viewport, Slicer::new())
}
