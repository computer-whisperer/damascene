//! **Meridian Slicer** — a static UI mock of a 3D-printer slicer, built
//! on the workbench vocabulary.
//!
//! Five regions, in the workbench's layered surface model:
//!
//! ```text
//! title_bar                                    (card,  30px)
//! ├ tool rail (activityBar.background, 48px) ┐
//! ├ viewport  (editor.background — the well) ├ the only Fill row
//! └ settings  (sideBar.background,   300px) ┘
//! status_bar                                   (card,  22px)
//! ```
//!
//! The viewport is the darkest thing on screen because it is painted
//! [`vs::EDITOR_BG`], and under [`theme::theme`] that key resolves to the
//! palette's `background` slot while every strip and panel resolves to
//! `card`, one step above it. Nothing here reaches for a raw color: the
//! well, the chrome and the rules are three token names, not three hex
//! literals, so the whole mock follows a palette swap.
//!
//! Everything is a stock widget or one of the four
//! [`damascene_workbench::chrome`] recipes. The settings panel is
//! `field_with(.., FieldOpts::inline_label(..))` — the inspector shape —
//! over `select_trigger`, `numeric_input` and `checkbox`; the rail is
//! `icon_button`s; the viewport overlay is `toggle_item`s on an
//! `editorWidget.background` slab.
//!
//! It is a **mock**: no state changes, no `on_event`. The one live
//! behavior is the rail's tooltips, which is why `build` ends in
//! `overlays(shell, [])` — the tooltip layer needs an `Axis::Overlay`
//! root or it becomes a third row and squashes the status bar.
//!
//! Run: `cargo run -p damascene-workbench --example slicer_v3`

use damascene_core::prelude::*;
use damascene_workbench::{chrome::*, theme, tokens as vs};

/// Activity-bar width. VS Code's rail is 48px and holds a 32px control
/// with 8px of air on each side; that is [`ComponentSize::Sm`].
const RAIL_WIDTH: f32 = 48.0;

/// One rail slot. Taller than the 32px button inside it so the active
/// accent rule reads as a *band*, the way VS Code's does.
const RAIL_SLOT_HEIGHT: f32 = 40.0;

/// The rail's active-item rule. Every slot spends this much of its left
/// edge on a border, active or not — only the color changes — so the
/// icons do not shift by a pixel as the selection moves.
const ACCENT_RULE: f32 = 2.0;

/// The settings dock. 300px is the brief; it is also what the inspector
/// rows below are proportioned for.
const PANEL_WIDTH: f32 = 300.0;

/// The inspector's label gutter — every control in the panel starts at
/// the same x, which is most of what makes a dense panel scan.
const LABEL_GUTTER: f32 = 104.0;

/// The rail's tools, in manipulation order. Typed [`IconName`]s rather
/// than strings: a typo is then a compile error instead of a runtime
/// `AlertCircle`.
const TOOLS: [(IconName, &str); 6] = [
    (IconName::Move, "Move"),
    (IconName::RotateCw, "Rotate"),
    (IconName::Scaling, "Scale"),
    (IconName::FlipHorizontal, "Mirror"),
    (IconName::Ruler, "Measure"),
    (IconName::Camera, "Camera"),
];

/// Which rail tool is lit.
const ACTIVE_TOOL: usize = 0;

struct MeridianSlicer {
    /// Every field here is display-only, so one empty selection serves
    /// all of them — `text_input`/`numeric_input` take the caret by
    /// reference and a mock never moves it.
    caret: Selection,
    model: &'static str,
}

impl MeridianSlicer {
    fn new() -> Self {
        Self {
            caret: Selection::default(),
            model: "bracket_v4_final.stl",
        }
    }

    // -----------------------------------------------------------------
    // Top strip.
    // -----------------------------------------------------------------

    /// App identity, the menus, and the one primary action in the shell.
    ///
    /// The menu triggers are stock `menubar_trigger`s dropped straight
    /// into `title_bar`'s leading slot with no `menubar()` box around
    /// them — that is the composition `chrome::title_bar` documents. They
    /// and the Slice button are all `Xxs`: the strip is 30px and an `Xs`
    /// control is 28px, which leaves no room for its focus ring.
    fn title(&self) -> El {
        let menu = |value: &str, label: &str| {
            menubar_trigger("mainmenu", value, label, false).size(ComponentSize::Xxs)
        };

        title_bar(
            [
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
            ],
            [
                text("Prusa MK4S · 0.4 mm").caption().muted(),
                button("Slice")
                    .key("slice")
                    .primary()
                    .size(ComponentSize::Xxs),
            ],
        )
    }

    // -----------------------------------------------------------------
    // Left rail.
    // -----------------------------------------------------------------

    /// One rail slot: a ghost `icon_button` centered in a band whose
    /// left border is the accent rule when the tool is active.
    ///
    /// The inactive band still draws its rule, painted
    /// `activityBar.background` — the rail's own fill — so it is the
    /// color that changes and not the geometry (see [`ACCENT_RULE`]).
    /// There is no `.selected()` here on purpose: that modifier gives a
    /// non-`item` control a tinted *fill*, and the activity bar's
    /// selection is a rule plus a brighter glyph, with no fill at all.
    fn tool(&self, glyph: IconName, label: &str, active: bool) -> El {
        let (accent, tint) = if active {
            (vs::TAB_ACTIVE_BORDER_TOP, vs::ACTIVITY_BAR_FG)
        } else {
            (vs::ACTIVITY_BAR_BG, vs::ACTIVITY_BAR_INACTIVE_FG)
        };

        row([icon_button(glyph)
            .key(format!("tool:{}", label.to_lowercase()))
            .ghost()
            .size(ComponentSize::Sm)
            .text_color(tint)
            .tooltip(label)])
        .justify(Justify::Center)
        .align(Align::Center)
        .width(Size::Fill(1.0))
        .height(Size::Fixed(RAIL_SLOT_HEIGHT))
        // `border_l()` is 1px; VS Code's rail rule is 2, which is what
        // `border_widths` is for (Tailwind's `border-l-2`).
        .border_widths(Sides::left(ACCENT_RULE))
        .border_color(accent)
    }

    fn rail(&self) -> El {
        let tools: Vec<El> = TOOLS
            .iter()
            .enumerate()
            .map(|(i, (glyph, label))| self.tool(*glyph, label, i == ACTIVE_TOOL))
            .collect();

        column(tools)
            .padding(Sides::xy(0.0, tokens::SPACE_1))
            .fill(vs::ACTIVITY_BAR_BG)
            .border_r()
            .border_color(vs::ACTIVITY_BAR_BORDER)
            .width(Size::Fixed(RAIL_WIDTH))
            .height(Size::Fill(1.0))
            .align(Align::Stretch)
    }

    // -----------------------------------------------------------------
    // Center — the viewport well.
    // -----------------------------------------------------------------

    /// The 3D view, mocked as its darkest-surface region.
    ///
    /// Two full-bleed layers in a `stack`: the placeholder caption,
    /// centered on both axes, and the corner overlay above it. An
    /// `Axis::Overlay` parent hands each child the whole rect, so the
    /// overlay's own padding — not a hand-computed offset — is what
    /// insets it from the corner.
    fn viewport(&self) -> El {
        let caption = column([
            text(self.model)
                .mono()
                .label()
                .text_color(vs::INPUT_PLACEHOLDER_FG),
            text("78.4 × 52.1 × 31.0 mm   ·   42,318 triangles")
                .caption()
                .text_color(vs::INPUT_PLACEHOLDER_FG),
        ])
        .gap(tokens::SPACE_1)
        .align(Align::Center)
        .justify(Justify::Center)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0));

        stack([caption, self.viewport_overlay()])
            .fill(vs::EDITOR_BG)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
    }

    /// Camera presets and the grid toggle, on a floating
    /// `editorWidget.background` slab — the one surface in this shell
    /// that rises *above* the well instead of sinking below it.
    fn viewport_overlay(&self) -> El {
        let preset = |value: &str, label: &str, on: bool| {
            toggle_item("camera", value, label, on).size(ComponentSize::Xxs)
        };

        let bar = row([
            preset("iso", "Iso", true),
            preset("front", "Front", false),
            preset("top", "Top", false),
            vertical_hairline().height(Size::Fixed(14.0)),
            toggle_item("overlays", "grid", "Grid", true).size(ComponentSize::Xxs),
            toggle_item("overlays", "supports", "Supports", false).size(ComponentSize::Xxs),
        ])
        .gap(tokens::SPACE_1)
        .align(Align::Center)
        .padding(Sides::all(tokens::SPACE_1))
        .fill(vs::EDITOR_WIDGET_BG)
        .stroke(vs::WIDGET_BORDER)
        .radius(vs::RADIUS)
        .width(Size::Hug)
        .height(Size::Hug);

        column([row([bar, spacer()]).width(Size::Fill(1.0))])
            .padding(Sides::all(tokens::SPACE_2))
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
    }

    // -----------------------------------------------------------------
    // Right — the settings dock.
    // -----------------------------------------------------------------

    /// A settings group: a `pane_header` strip over its rows.
    ///
    /// The header is the uppercase, ruled, 22px `sideBarSectionHeader`
    /// bar, so the groups need no boxes of their own — the rule under
    /// each header is the separator, which is the whole point of the
    /// layered model (`section_header` is the *document* shape and would
    /// be wrong here).
    fn group<I, E>(title: &str, rows: I) -> El
    where
        I: IntoIterator<Item = E>,
        E: Into<El>,
    {
        column([
            pane_header(title, Vec::<El>::new()),
            column(rows)
                .gap(tokens::SPACE_2)
                .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_2))
                .width(Size::Fill(1.0))
                .align(Align::Stretch),
        ])
        .width(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    /// One inspector row — label in the shared gutter, control filling
    /// the rest.
    fn row_for(label: &str, control: El) -> El {
        field_with(label, control, FieldOpts::default().inline_label(LABEL_GUTTER))
    }

    /// A read-only numeric field with its unit inside the trough.
    ///
    /// `NumericInputOpts::suffix` is why there is no `input_group` here:
    /// "a number with a unit and a stepper" is one widget, and the unit
    /// sits inside the field's focus ring rather than beside it.
    fn number(&self, key: &str, value: &str, unit: &str) -> El {
        numeric_input(
            key,
            value,
            &self.caret,
            NumericInputOpts::default().stacked().suffix(unit),
        )
    }

    fn settings(&self) -> El {
        let quality = Self::group(
            "QUALITY",
            [
                Self::row_for(
                    "Layer height",
                    select_trigger("quality:layer", "0.20 mm — Quality"),
                ),
                Self::row_for("Wall count", self.number("quality:walls", "3", "")),
            ],
        );

        let infill = Self::group(
            "INFILL",
            [
                Self::row_for("Density", self.number("infill:density", "18", "%")),
                Self::row_for("Pattern", select_trigger("infill:pattern", "Gyroid")),
            ],
        );

        let material = Self::group(
            "MATERIAL",
            [
                Self::row_for(
                    "Filament",
                    select_trigger("material:filament", "PLA — Galaxy Black"),
                ),
                Self::row_for("Nozzle", self.number("material:nozzle", "215", "°C")),
                Self::row_for("Bed", self.number("material:bed", "60", "°C")),
            ],
        );

        let supports = Self::group(
            "SUPPORTS",
            [
                row([
                    checkbox("supports:enable", true),
                    text("Generate support structures").caption(),
                ])
                .gap(tokens::SPACE_2)
                .align(Align::Center)
                .width(Size::Fill(1.0)),
                Self::row_for("Overhang", self.number("supports:overhang", "55", "°")),
                Self::row_for(
                    "Placement",
                    select_trigger("supports:placement", "Touching build plate"),
                ),
            ],
        );

        // `scroll` rather than a plain column: the dock is the one region
        // whose content can outgrow the window when a group is added, and
        // it clips on its own (a bare `.scrollable()` does not).
        scroll([quality, infill, material, supports])
            .key("settings")
            .gap(tokens::SPACE_1)
            .fill(vs::SIDE_BAR_BG)
            .border_l()
            .border_color(vs::SIDE_BAR_BORDER)
            .width(Size::Fixed(PANEL_WIDTH))
            .height(Size::Fill(1.0))
            .align(Align::Stretch)
    }

    // -----------------------------------------------------------------
    // Bottom strip.
    // -----------------------------------------------------------------

    /// Left: what the app is doing and what it is holding. Right: the
    /// two numbers a slicer exists to produce, as chips — a chip is a
    /// *value*, plain caption text is a label, and keeping the two
    /// distinct is what stops the bar reading as a sentence.
    fn status(&self) -> El {
        status_bar(
            [
                text("Ready").caption(),
                text(self.model).caption(),
                chip("sliced").success(),
            ],
            [
                text("Print time").caption(),
                chip("2 h 14 m"),
                text("Filament").caption(),
                chip("38.6 g · 12.94 m"),
            ],
        )
    }
}

impl App for MeridianSlicer {
    fn build(&self, _cx: &BuildCx) -> El {
        // A bare column, not `page()`: a workbench shell is full-bleed,
        // and window padding would float the whole instrument on a
        // margin.
        let shell = column([
            self.title(),
            row([self.rail(), self.viewport(), self.settings()])
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0))
                .align(Align::Stretch),
            self.status(),
        ])
        .fill(vs::EDITOR_BG)
        .align(Align::Stretch);

        // The rail's tooltips need the root to stack its children; see
        // the module docs.
        overlays(shell, [])
    }

    fn theme(&self) -> Theme {
        theme::theme()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 1280.0, 800.0);
    damascene_winit_wgpu::run("Meridian Slicer", viewport, MeridianSlicer::new())
}
