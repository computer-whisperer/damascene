//! "Meridian Slicer" — a static workbench-genre mock of a 3D printer
//! slicer, at a fixed 1280×800.
//!
//! Same claim as `examples/shell.rs`, in a different product genre. A
//! slicer is a CAD-adjacent instrument rather than an editor, and it
//! still lands on the workbench vocabulary without a bespoke surface:
//! a slim title/menu strip, a narrow icon tool rail, a dark 3D canvas
//! with a small in-canvas overlay, a dense 300px settings panel, and a
//! status bar.
//!
//! Everything is stock — `menubar_trigger`, `button`, `icon_button`,
//! `select_trigger`, `numeric_input`, `input_group`, `checkbox`,
//! `field_row`, `toggle_item` — plus the four
//! [`damascene_workbench::chrome`] recipes (`status_bar`, `chip`,
//! `pane_header`, `hairline`), rendered through
//! [`damascene_workbench::theme::theme`].
//!
//! This is a **static mock**: every control renders at a real value and
//! takes focus, but there is no `on_event` impl. Nothing slices.
//!
//! Run: `cargo run -p damascene-workbench --example slicer_v2`

use damascene_core::prelude::*;
use damascene_workbench::{chrome::*, theme, tokens as vs};

// ---------------------------------------------------------------------
// The three colors the workbench vocabulary has no key for.
// ---------------------------------------------------------------------

/// The 3D canvas floor. Deliberately below every workbench surface: a
/// viewport is not chrome, and the crate's ramp bottoms out at the
/// content well, so there is no token for "under the lowest panel".
const CANVAS_BG: Color = Color::srgb_u8(17, 17, 19);

/// The build-plate footprint, drawn as a wash rather than as geometry.
const PLATE_LINE: Color = Color::srgb_u8a(255, 255, 255, 20);

/// The centered canvas placeholder — legible, but well under the
/// chrome's own foreground so the canvas still reads as empty.
const CANVAS_HINT_FG: Color = Color::srgb_u8(126, 131, 139);

/// Sub-line under the placeholder, one step dimmer again.
const CANVAS_SUBHINT_FG: Color = Color::srgb_u8(88, 92, 99);

/// A bordered side that should not paint — the inactive tool rail's
/// accent slot, kept in the tree so active and inactive rail items have
/// the identical content box and the icons do not shift by a pixel.
const NO_ACCENT: Color = Color::srgb_u8a(0, 0, 0, 0);

const MENU_KEY: &str = "menu";
const TOOL_KEY: &str = "tool";
const VIEW_KEY: &str = "view";

/// Width of the tool rail, matching VS Code's activity bar rung.
const RAIL_WIDTH: f32 = 44.0;

/// Width of the settings panel.
const PANEL_WIDTH: f32 = 300.0;

/// Every control in the settings panel trails at this width, so the
/// sections share one column edge.
const CONTROL_WIDTH: f32 = 136.0;

/// Chrome-rung control height — one step under the theme's 28px so
/// buttons clear the 30px title strip with a border of air.
const BAR_CONTROL_HEIGHT: f32 = 22.0;

struct Slicer {
    /// Every text/numeric field in the panel shares the app's single
    /// selection slot, as `Selection`'s contract requires. Nothing
    /// writes to it in this mock, so it stays empty.
    selection: Selection,
    active_tool: &'static str,
    active_view: &'static str,
    show_grid: bool,
    supports: bool,
    walls: String,
    top_bottom: String,
    density: String,
    nozzle_temp: String,
    bed_temp: String,
    overhang: String,
    z_distance: String,
    brim_width: String,
}

impl Slicer {
    fn new() -> Self {
        Self {
            selection: Selection::default(),
            active_tool: "move",
            active_view: "iso",
            show_grid: true,
            supports: true,
            walls: "3".into(),
            top_bottom: "4".into(),
            density: "20".into(),
            nozzle_temp: "215".into(),
            bed_temp: "60".into(),
            overhang: "55".into(),
            z_distance: "0.2".into(),
            brim_width: "5.0".into(),
        }
    }

    // -----------------------------------------------------------------
    // Title / menu strip
    // -----------------------------------------------------------------

    /// App name, the menu, and the one primary action. `titleBar.*`
    /// with an under-rule — the `border_b` case per-side borders exist
    /// for.
    fn title_bar(&self) -> El {
        let menus = ["File", "Edit", "View", "Help"].map(|label| {
            menubar_trigger(MENU_KEY, label.to_lowercase(), label, false)
                .height(Size::Fixed(BAR_CONTROL_HEIGHT))
                .padding(Sides::xy(tokens::SPACE_2, 0.0))
        });

        row([
            text("Meridian Slicer")
                .caption()
                .font_weight(FontWeight::Medium)
                .text_color(vs::TITLE_BAR_ACTIVE_FG),
            row(menus).gap(tokens::SPACE_0).align(Align::Center),
            spacer(),
            text("Meridian X1 · 0.4 mm nozzle")
                .caption()
                .text_color(vs::DESCRIPTION_FG),
            button("Slice")
                .key("slice")
                .primary()
                .height(Size::Fixed(BAR_CONTROL_HEIGHT)),
        ])
        .gap(tokens::SPACE_3)
        .fill(vs::TITLE_BAR_ACTIVE_BG)
        .border_b()
        .border_color(vs::TITLE_BAR_BORDER)
        .padding(Sides::x(tokens::SPACE_2))
        .height(Size::Fixed(vs::TITLE_BAR_HEIGHT))
        .width(Size::Fill(1.0))
        .align(Align::Center)
    }

    // -----------------------------------------------------------------
    // Tool rail
    // -----------------------------------------------------------------

    /// Six manipulators on `activityBar.background`, one of them
    /// current. The rail separates itself from the canvas with a right
    /// border, not a gap.
    fn tool_rail(&self) -> El {
        let tools = [
            ("move", IconName::Move),
            ("rotate", IconName::RotateCw),
            ("scale", IconName::Scaling),
            ("mirror", IconName::FlipHorizontal),
            ("measure", IconName::Ruler),
            ("camera", IconName::Camera),
        ];

        column(tools.map(|(name, glyph)| self.tool(name, glyph)))
            .gap(tokens::SPACE_1)
            .padding(Sides::y(tokens::SPACE_2))
            .fill(vs::ACTIVITY_BAR_BG)
            .border_r()
            .border_color(vs::ACTIVITY_BAR_BORDER)
            .width(Size::Fixed(RAIL_WIDTH))
            .height(Size::Fill(1.0))
            .align(Align::Center)
    }

    /// One rail item: a ghost `icon_button` in a slot that owns the
    /// left accent bar. The accent is a border on the slot rather than
    /// on the button so it runs the full item height and is unaffected
    /// by the button's hover fill.
    fn tool(&self, name: &'static str, glyph: IconName) -> El {
        let active = self.active_tool == name;
        let button = icon_button(glyph)
            .key(format!("{TOOL_KEY}:{name}"))
            .width(Size::Fixed(28.0))
            .height(Size::Fixed(28.0));
        let button = if active {
            button.current()
        } else {
            button.ghost().text_color(vs::ACTIVITY_BAR_INACTIVE_FG)
        };

        row([button])
            .border_l()
            .border_color(if active { vs::FOCUS_BORDER } else { NO_ACCENT })
            .width(Size::Fixed(RAIL_WIDTH))
            .height(Size::Fixed(32.0))
            .align(Align::Center)
            .justify(Justify::Center)
    }

    // -----------------------------------------------------------------
    // Canvas
    // -----------------------------------------------------------------

    /// The 3D viewport, as three overlay layers over one dark fill:
    /// the build-plate footprint, the centered model placeholder, and
    /// the in-canvas control cluster pinned to the top-left corner.
    /// Every layer is `Fill`/`Fill` so it spans the canvas and places
    /// its own content — an overlay container aligns all its children
    /// alike.
    fn canvas(&self) -> El {
        stack([
            self.build_plate(),
            self.model_placeholder(),
            self.canvas_overlay(),
        ])
        .fill(CANVAS_BG)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
    }

    fn build_plate(&self) -> El {
        column([column(Vec::<El>::new())
            .stroke(PLATE_LINE)
            .stroke_width(1.0)
            .radius(vs::RADIUS)
            .width(Size::Fixed(560.0))
            .height(Size::Fixed(340.0))])
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Center)
        .justify(Justify::Center)
    }

    fn model_placeholder(&self) -> El {
        column([
            text("benchy_hull_v3.stl")
                .label()
                .text_color(CANVAS_HINT_FG),
            text("142.0 × 96.4 × 48.2 mm  ·  1 object  ·  scaled 100%")
                .caption()
                .text_color(CANVAS_SUBHINT_FG),
        ])
        .gap(tokens::SPACE_1)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Center)
        .justify(Justify::Center)
    }

    /// Camera presets and a grid toggle, in a floating widget-surface
    /// cluster. `editorWidget.background` is the workbench key for
    /// exactly this: a small panel laid *over* content.
    fn canvas_overlay(&self) -> El {
        let cluster = row([
            toggle_item(VIEW_KEY, "iso", "Iso", self.active_view == "iso")
                .height(Size::Fixed(BAR_CONTROL_HEIGHT)),
            toggle_item(VIEW_KEY, "top", "Top", self.active_view == "top")
                .height(Size::Fixed(BAR_CONTROL_HEIGHT)),
            toggle_item(VIEW_KEY, "front", "Front", self.active_view == "front")
                .height(Size::Fixed(BAR_CONTROL_HEIGHT)),
            vertical_separator().height(Size::Fixed(BAR_CONTROL_HEIGHT)),
            toggle("grid", self.show_grid, "Grid").height(Size::Fixed(BAR_CONTROL_HEIGHT)),
        ])
        .gap(tokens::SPACE_1)
        .padding(tokens::SPACE_1)
        .fill(vs::EDITOR_WIDGET_BG)
        .stroke(vs::WIDGET_BORDER)
        .stroke_width(1.0)
        .radius(vs::RADIUS)
        .width(Size::Hug)
        .align(Align::Center);

        column([cluster])
            .padding(tokens::SPACE_3)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Start)
            .justify(Justify::Start)
    }

    // -----------------------------------------------------------------
    // Settings panel
    // -----------------------------------------------------------------

    /// The right-hand inspector on `sideBar.background`, capped by a
    /// pane header and separated from the canvas by a left border.
    fn settings_panel(&self) -> El {
        column([
            pane_header("PRINT SETTINGS", [chip("modified")]),
            column([
                self.quality(),
                self.infill(),
                self.material(),
                self.supports(),
                self.adhesion(),
            ])
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Stretch)
            .scrollable(),
        ])
        .fill(vs::SIDE_BAR_BG)
        .border_l()
        .border_color(vs::SIDE_BAR_BORDER)
        .width(Size::Fixed(PANEL_WIDTH))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    fn quality(&self) -> El {
        section(
            "QUALITY",
            [
                setting("Layer height", select_trigger("layer-height", "0.20 mm")),
                setting("Wall line count", self.count("walls", &self.walls)),
                setting(
                    "Top / bottom layers",
                    self.count("top-bottom", &self.top_bottom),
                ),
            ],
        )
    }

    fn infill(&self) -> El {
        section(
            "INFILL",
            [
                setting("Density", self.unit_field("density", &self.density, "%")),
                setting("Pattern", select_trigger("infill-pattern", "Gyroid")),
            ],
        )
    }

    fn material(&self) -> El {
        section(
            "MATERIAL",
            [
                setting("Material", select_trigger("material", "PLA Matte")),
                setting(
                    "Nozzle temperature",
                    self.unit_field("nozzle-temp", &self.nozzle_temp, "°C"),
                ),
                setting(
                    "Build plate temp.",
                    self.unit_field("bed-temp", &self.bed_temp, "°C"),
                ),
            ],
        )
    }

    fn supports(&self) -> El {
        section(
            "SUPPORTS",
            [
                setting("Generate supports", checkbox("supports", self.supports)),
                setting(
                    "Overhang angle",
                    self.unit_field("overhang", &self.overhang, "°"),
                ),
                setting(
                    "Z distance",
                    self.unit_field("z-distance", &self.z_distance, "mm"),
                ),
            ],
        )
    }

    fn adhesion(&self) -> El {
        section(
            "BUILD PLATE",
            [
                setting("Adhesion type", select_trigger("adhesion", "Brim")),
                setting(
                    "Brim width",
                    self.unit_field("brim-width", &self.brim_width, "mm"),
                ),
            ],
        )
    }

    /// A unitless integer field: the stock stacked-chevron
    /// `numeric_input`.
    ///
    /// Wrapped in an `input_group` rather than used bare, because a
    /// bare `numeric_input` puts the trough on its *inner* field and
    /// hangs the steppers outside it — so its visible right edge lands
    /// ~20px short of the `select_trigger`s and unit fields sharing the
    /// column. The group re-hosts the trough on the whole control (it
    /// de-chromes `numeric_input` children by design) and the column
    /// lines up.
    fn count(&self, key: &str, value: &str) -> El {
        input_group([numeric_input(
            key,
            value,
            &self.selection,
            NumericInputOpts::default().min(1.0).max(99.0).stacked(),
        )
        .width(Size::Fill(1.0))])
        .width(Size::Fixed(CONTROL_WIDTH))
    }

    /// A field that carries a unit. `numeric_input` has no unit slot,
    /// so this is the `input_group` composition instead: a bare
    /// `text_input` in the group's trough with an `input_group_text`
    /// suffix cell. It trades the steppers for the unit.
    fn unit_field(&self, key: &str, value: &str, unit: &str) -> El {
        input_group([
            text_input_with(
                key,
                value,
                &self.selection,
                TextInputOpts::default().tabular_numerals(),
            ),
            input_group_text(unit),
        ])
        .width(Size::Fixed(CONTROL_WIDTH))
    }
}

/// A titled block inside the settings panel: the uppercase pane header
/// strip, then the rows on the panel's own fill.
fn section<I, E>(title: &str, rows: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column([
        pane_header(title, Vec::<El>::new()),
        column(rows)
            .gap(tokens::SPACE_2)
            .padding(tokens::SPACE_3)
            .width(Size::Fill(1.0))
            .align(Align::Stretch),
    ])
    .width(Size::Fill(1.0))
    .height(Size::Hug)
    .align(Align::Stretch)
}

/// One inspector row — the stock `field`, in shadcn's horizontal
/// orientation, which is documented as exactly this settings-row shape:
/// the label takes the flex and ellipsizes, the control trails right at
/// the panel's shared column width.
///
/// Not `field_row`: that one hugs the label and pushes the control with
/// a `spacer`, so a fixed-width control leaves the panel entirely once
/// the label is long enough ("Build plate temp.").
fn setting(label: &str, control: impl Into<El>) -> El {
    field_with(label, control, FieldOpts::default().horizontal())
}

impl App for Slicer {
    fn build(&self, _cx: &BuildCx) -> El {
        // A bare column root, not `page()`: a workbench shell is
        // full-bleed by definition, and window padding would float the
        // whole instrument on a margin.
        column([
            self.title_bar(),
            row([self.tool_rail(), self.canvas(), self.settings_panel()])
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0))
                .align(Align::Stretch),
            status_bar(
                [
                    text("Ready").caption(),
                    text("benchy_hull_v3.stl").caption().text_color(vs::DESCRIPTION_FG),
                    chip("1 object"),
                ],
                [
                    text("Estimated").caption().text_color(vs::DESCRIPTION_FG),
                    chip("2 h 14 m"),
                    chip("18.4 g · 6.11 m"),
                ],
            ),
        ])
        .fill(vs::EDITOR_BG)
        .align(Align::Stretch)
    }

    // Without this, stock controls render at shadcn's 36px on a flat
    // single-surface palette.
    fn theme(&self) -> Theme {
        theme::theme()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 1280.0, 800.0);
    damascene_winit_wgpu::run("Meridian Slicer", viewport, Slicer::new())
}
