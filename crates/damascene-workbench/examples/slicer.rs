//! "Meridian Slicer" — a static workbench-genre mock of a 3D printer
//! slicer.
//!
//! A second read on the same claim `shell.rs` makes, in a different
//! product genre: a slicer is a CAD-adjacent tool, not an editor, and it
//! still lands on the workbench vocabulary without a single bespoke
//! surface. Title/menu strip, a narrow tool rail, a dark 3D canvas, a
//! dense right-hand settings panel, a status bar.
//!
//! Everything here is stock — `button`, `select_trigger`,
//! `numeric_input`, `checkbox`, `field_row`, `icon_button` — plus the
//! four [`damascene_workbench::chrome`] recipes (`status_bar`, `chip`,
//! `pane_header`, `hairline`), rendered through
//! [`damascene_workbench::theme::theme`].
//!
//! This is a **static mock**: the controls render at their real values
//! and take focus, but `on_event` is deliberately empty. Nothing slices.
//!
//! Run: `cargo run -p damascene-workbench --example slicer`

use damascene_core::prelude::*;
use damascene_workbench::{chrome::*, theme, tokens as vs};

/// The 3D canvas. Darker than every workbench surface on purpose: a
/// viewport is not chrome, and the Dark Modern ramp bottoms out at
/// `sideBar.background` (`#181818`), so there is no token for "below the
/// lowest panel". Hand-picked one step under it.
const VIEWPORT_BG: Color = Color::srgb_u8(15, 15, 15);

/// The build-plate grid, drawn as a wash rather than as geometry.
const PLATE_LINE: Color = Color::srgb_u8a(255, 255, 255, 12);

/// Edge of one square activity-bar tool, and so the width of the bar
/// itself and of the rule that splits its tool groups.
const TOOL_SIZE: f32 = 38.0;

const MENU_KEY: &str = "menu";

struct Slicer {
    /// Every text/numeric field in the panel shares the app's single
    /// selection slot, as `Selection`'s contract requires. Nothing
    /// writes to it in this mock, so it stays empty.
    selection: Selection,
    active_tool: &'static str,
    model: &'static str,
}

impl Slicer {
    fn new() -> Self {
        Self {
            selection: Selection::default(),
            active_tool: "rotate",
            model: "bracket_v7_rev-c.3mf",
        }
    }

    // -----------------------------------------------------------------
    // Title / menu strip.
    // -----------------------------------------------------------------

    /// `titleBar.activeBackground` with an under-rule, the app name, the
    /// menu, and the one primary action in the whole window.
    fn title_bar(&self) -> El {
        let menu = |value: &str, label: &str| {
            menubar_trigger(MENU_KEY, value, label, false).height(Size::Fixed(22.0))
        };

        row([
            text("Meridian Slicer")
                .caption()
                .font_weight(FontWeight::Semibold)
                .text_color(vs::TITLE_BAR_ACTIVE_FG),
            row([
                menu("file", "File"),
                menu("edit", "Edit"),
                menu("view", "View"),
                menu("help", "Help"),
            ])
            .gap(tokens::SPACE_0)
            .align(Align::Center),
            spacer(),
            text("Prusa MK4 · 0.4 mm").caption(),
            button("Slice")
                .key("slice")
                .primary()
                .size(ComponentSize::Xs)
                .height(Size::Fixed(22.0)),
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
    // Tool rail.
    // -----------------------------------------------------------------

    /// VS Code's activity bar, re-cast as a transform-gizmo rail. The
    /// active tool takes `activityBar.foreground` plus a `focusBorder`
    /// left rule; the rest sit at `activityBar.inactiveForeground`.
    ///
    /// Every gizmo glyph is a built-in: the vocabulary carries `move`,
    /// `rotate-cw`, `scaling`, `flip-horizontal`, `ruler` and `camera`,
    /// so the rail needs no app-supplied SVG at all.
    fn tool_rail(&self) -> El {
        let tool = |id: &'static str, glyph: IconName, label: &str| {
            let active = self.active_tool == id;
            let b = icon_button(glyph)
                .key(format!("tool:{id}"))
                .tooltip(label.to_string())
                .ghost()
                .icon_size(tokens::ICON_MD)
                .width(Size::Fixed(TOOL_SIZE))
                .height(Size::Fixed(TOOL_SIZE))
                .radius(0.0);
            if active {
                b.fill(vs::LIST_INACTIVE_SELECTION_BG)
                    .text_color(vs::ACTIVITY_BAR_FG)
                    .border_l()
                    .border_color(vs::FOCUS_BORDER)
            } else {
                b.text_color(vs::ACTIVITY_BAR_INACTIVE_FG)
            }
        };

        column([
            tool("move", IconName::Move, "Move"),
            tool("rotate", IconName::RotateCw, "Rotate"),
            tool("scale", IconName::Scaling, "Scale"),
            tool("mirror", IconName::FlipHorizontal, "Mirror"),
            // The bar centers its (fixed-width) tools, and a centered
            // parent resolves cross-axis `Size::Fill` to the child's
            // zero intrinsic — `hairline()`'s full width would paint
            // nothing here, so the rule spans the bar explicitly.
            hairline().width(Size::Fixed(TOOL_SIZE)),
            tool("measure", IconName::Ruler, "Measure"),
            tool("camera", IconName::Camera, "Camera"),
        ])
        .gap(tokens::SPACE_0)
        .padding(Sides::y(tokens::SPACE_1))
        .fill(vs::ACTIVITY_BAR_BG)
        .border_r()
        .border_color(vs::ACTIVITY_BAR_BORDER)
        .width(Size::Fixed(TOOL_SIZE))
        .height(Size::Fill(1.0))
        .align(Align::Center)
    }

    // -----------------------------------------------------------------
    // Viewport.
    // -----------------------------------------------------------------

    /// The 3D canvas placeholder: darkest surface, a corner overlay of
    /// camera-preset and grid chips, and a dim centered model caption
    /// where the mesh would be.
    fn viewport(&self) -> El {
        column([
            row([spacer(), self.viewport_overlay()]).width(Size::Fill(1.0)),
            spacer(),
            column([
                text(self.model)
                    .body()
                    .text_color(Color::srgb_u8(88, 88, 88))
                    .font_weight(FontWeight::Medium),
                text("184.2 × 96.0 × 41.8 mm · 1 instance")
                    .caption()
                    .text_color(Color::srgb_u8(72, 72, 72)),
                text("— viewport —")
                    .caption()
                    .mono()
                    .text_color(Color::srgb_u8(56, 56, 56)),
            ])
            .gap(tokens::SPACE_1)
            .width(Size::Fill(1.0))
            .align(Align::Center),
            spacer(),
            // Stand-in for the build plate: one wash-colored rule where
            // the plate's front edge would meet the camera.
            divider()
                .width(Size::Fill(1.0))
                .height(Size::Fixed(vs::HAIRLINE))
                .fill(PLATE_LINE),
            row([
                text("build plate 250 × 210 mm").caption(),
                spacer(),
                text("perspective · 1:1.4").caption(),
            ])
            .width(Size::Fill(1.0))
            .padding(Sides::xy(tokens::SPACE_1, tokens::SPACE_1))
            .align(Align::Center),
        ])
        .gap(tokens::SPACE_2)
        .padding(tokens::SPACE_2)
        .fill(VIEWPORT_BG)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    /// The floating in-canvas control cluster. `quickInput.background`
    /// with `SHADOW_MD` — the crate's rule is that docked chrome is flat
    /// and only overlays keep a shadow, and this is an overlay.
    fn viewport_overlay(&self) -> El {
        let preset = |id: &str, label: &str, current: bool| {
            let b = button(label)
                .key(format!("view:{id}"))
                .ghost()
                .size(ComponentSize::Xs)
                .height(Size::Fixed(20.0))
                .padding(Sides::x(tokens::SPACE_2));
            if current { b.current() } else { b }
        };

        column([
            row([
                preset("top", "Top", false),
                preset("front", "Front", true),
                preset("iso", "Iso", false),
            ])
            .gap(tokens::SPACE_1)
            .align(Align::Center),
            row([
                chip("GRID ON"),
                chip("SUPPORTS"),
                text("G").caption().mono(),
            ])
            .gap(tokens::SPACE_1)
            .align(Align::Center),
        ])
        .gap(tokens::SPACE_1)
        .padding(tokens::SPACE_1)
        .fill(vs::QUICK_INPUT_BG)
        .stroke(vs::WIDGET_BORDER)
        .radius(vs::RADIUS)
        .shadow(tokens::SHADOW_MD)
        .width(Size::Hug)
        .align(Align::Stretch)
    }

    // -----------------------------------------------------------------
    // Settings panel.
    // -----------------------------------------------------------------

    /// A pane-header section over a padded column of `field_row`s. The
    /// header is `pane_header`, i.e. VS Code's `sideBarSectionHeader` —
    /// the uppercase 22px strip, not a card title.
    fn section<I, E>(title: &str, trailing: Vec<El>, rows: I) -> El
    where
        I: IntoIterator<Item = E>,
        E: Into<El>,
    {
        column([
            pane_header(title.to_string(), trailing),
            column(rows)
                .gap(tokens::SPACE_2)
                .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_2))
                .width(Size::Fill(1.0)),
        ])
        .width(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    /// A select sized for the panel's label/control split — the stock
    /// trigger is `Fill(1.0)` by default, which would fight `field_row`'s
    /// spacer.
    fn panel_select(key: &str, label: &str) -> El {
        select_trigger(key.to_string(), label.to_string())
            .size(ComponentSize::Xs)
            .width(Size::Fixed(150.0))
    }

    /// A numeric field in the stacked-chevron layout: the flanked
    /// `[−] field [+]` default is 144px wide and does not fit a 300px
    /// panel beside a label.
    fn panel_number(&self, key: &str, value: &str, unit: &str) -> El {
        let field = numeric_input(
            key,
            value,
            &self.selection,
            NumericInputOpts::default().stacked(),
        )
        .size(ComponentSize::Xs)
        .width(Size::Fixed(if unit.is_empty() { 96.0 } else { 84.0 }));

        if unit.is_empty() {
            field
        } else {
            row([
                field,
                text(unit.to_string())
                    .caption()
                    .width(Size::Fixed(28.0))
                    .text_color(vs::DESCRIPTION_FG),
            ])
            .gap(tokens::SPACE_1)
            .align(Align::Center)
            .width(Size::Hug)
        }
    }

    fn settings_panel(&self) -> El {
        column([
            pane_header("PRINT SETTINGS", [chip("MODIFIED")]),
            column([
                Self::section(
                    "QUALITY",
                    vec![chip("0.20")],
                    [
                        field_row("Layer height", Self::panel_select("q:layer", "0.20 mm")),
                        field_row("First layer", Self::panel_select("q:first", "0.25 mm")),
                        field_row("Wall count", self.panel_number("q:walls", "3", "")),
                        field_row("Top / bottom", self.panel_number("q:skins", "5", "")),
                    ],
                ),
                hairline(),
                Self::section(
                    "INFILL",
                    Vec::new(),
                    [
                        field_row("Density", self.panel_number("i:density", "22", "%")),
                        field_row("Pattern", Self::panel_select("i:pattern", "Gyroid")),
                        field_row("Anchor length", self.panel_number("i:anchor", "2.5", "mm")),
                    ],
                ),
                hairline(),
                Self::section(
                    "MATERIAL",
                    vec![chip("PLA")],
                    [
                        field_row(
                            "Filament",
                            Self::panel_select("m:filament", "Prusament PLA"),
                        ),
                        field_row("Nozzle temp", self.panel_number("m:nozzle", "215", "°C")),
                        field_row("Bed temp", self.panel_number("m:bed", "60", "°C")),
                        field_row("Flow", self.panel_number("m:flow", "98", "%")),
                    ],
                ),
                hairline(),
                Self::section(
                    "SUPPORTS",
                    Vec::new(),
                    [
                        field_row("Generate supports", checkbox("s:enable", true)),
                        field_row("Overhang angle", self.panel_number("s:angle", "55", "°")),
                        field_row("Z distance", self.panel_number("s:zgap", "0.15", "mm")),
                        field_row("Build plate only", checkbox("s:plate", false)),
                    ],
                ),
            ])
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Stretch)
            .scrollable(),
        ])
        .fill(vs::SIDE_BAR_BG)
        .border_l()
        .border_color(vs::SIDE_BAR_BORDER)
        .width(Size::Fixed(300.0))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
    }
}

impl App for Slicer {
    fn build(&self, _cx: &BuildCx) -> El {
        // A bare column root, not `page()`: full-bleed by definition, per
        // `shell.rs`. Window padding would float the instrument.
        column([
            self.title_bar(),
            row([self.tool_rail(), self.viewport(), self.settings_panel()])
                .height(Size::Fill(1.0))
                .width(Size::Fill(1.0))
                .align(Align::Stretch),
            status_bar(
                [
                    text("Ready").caption(),
                    text(self.model).caption(),
                    text("sliced 3 min ago").caption(),
                ],
                [
                    text("Estimated").caption().text_color(vs::DESCRIPTION_FG),
                    chip("4 h 12 m"),
                    text("Filament").caption().text_color(vs::DESCRIPTION_FG),
                    chip("28.4 m · 84 g"),
                    text("MK4 · 0.4").caption(),
                ],
            ),
        ])
        .align(Align::Stretch)
        .fill(vs::EDITOR_BG)
    }

    // Static mock: the controls render and take focus, nothing folds.
    fn on_event(&mut self, _event: UiEvent, _cx: &EventCx) {}

    // Without this, stock controls render in shadcn zinc at 36px.
    fn theme(&self) -> Theme {
        theme::theme()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 1280.0, 800.0);
    damascene_winit_wgpu::run("Meridian Slicer", viewport, Slicer::new())
}
