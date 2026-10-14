//! A VS Code-ish application shell on the workbench theme.
//!
//! Title strip, side bar with a file list, editor tab strip, main area,
//! status bar. Everything but the four [`damascene_workbench::chrome`]
//! recipes is a **stock** damascene widget — `sidebar_menu_button`,
//! `editor_tabs`, `button`, `card` — rendered through
//! [`damascene_workbench::theme::theme`]. That is the whole claim of the
//! crate: the same vocabulary, re-skinned into an instrument.
//!
//! Comment out the `theme()` impl at the bottom to see what the identical
//! tree looks like on stock shadcn. Every one of the four signals in
//! `docs/WORKBENCH_VISION.md` moves at once — controls grow to 36px,
//! corners round to 7–12px, the layered fills collapse into one
//! background with outlined floating cards, and the regions drift apart.
//!
//! Try:
//! - Click a file in the side bar, or a tab. Middle-click a tab to close
//!   it; `+` opens a new one.
//! - Tab through the controls — the focus ring is `focusBorder`
//!   (`#0078D4`), VS Code's blue, because it backs the `ring` slot.
//! - Rest the pointer on `main*` in the status bar. That tooltip is why
//!   `build` ends in `overlays(shell, [])` — see the comment there.
//!
//! Run: `cargo run -p damascene-workbench --example shell`

use damascene_core::prelude::*;
use damascene_workbench::{chrome::*, theme, tokens as vs};

const TABS_KEY: &str = "editors";

struct Shell {
    files: Vec<&'static str>,
    open: Vec<String>,
    active: String,
}

impl Shell {
    fn new() -> Self {
        Self {
            files: vec![
                "main.rs",
                "theme.rs",
                "tokens.rs",
                "chrome.rs",
                "Cargo.toml",
                "README.md",
            ],
            open: vec!["main.rs".into(), "theme.rs".into(), "chrome.rs".into()],
            active: "theme.rs".into(),
        }
    }

    /// The window title strip — `titleBar.activeBackground` with an
    /// under-rule, exactly the `border_b` case per-side borders were
    /// added for.
    fn title_strip(&self) -> El {
        row([
            text("damascene — workbench")
                .caption()
                .text_color(vs::TITLE_BAR_ACTIVE_FG),
            spacer(),
            text("File  Edit  View  Go  Run")
                .caption()
                .text_color(vs::DESCRIPTION_FG),
        ])
        .fill(vs::TITLE_BAR_ACTIVE_BG)
        .border_b()
        .border_color(vs::TITLE_BAR_BORDER)
        .padding(Sides::x(tokens::SPACE_2))
        .height(Size::Fixed(vs::TITLE_BAR_HEIGHT))
        .width(Size::Fill(1.0))
        .align(Align::Center)
    }

    /// The explorer rail. Note what is *not* here: no `.radius(0)`, no
    /// density overrides, no hand-rolled hairline stack. The panel is a
    /// plain column on `sideBar.background` with a right border, and the
    /// rows are stock `sidebar_menu_button`s.
    fn side_bar(&self) -> El {
        let rows: Vec<El> = self
            .files
            .iter()
            .map(|f| sidebar_menu_button(*f, self.active == *f).key(format!("file:{f}")))
            .collect();

        column([
            pane_header("EXPLORER", [chip(format!("{}", self.open.len()))]),
            column([sidebar_menu(rows)])
                .padding(tokens::SPACE_2)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0))
                .scrollable(),
        ])
        .fill(vs::SIDE_BAR_BG)
        .border_r()
        .border_color(vs::SIDE_BAR_BORDER)
        .width(Size::Fixed(220.0))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    /// Tab strip plus editor body. The tab strip sits in its
    /// `editorGroupHeader.tabsBackground` trough; the active tab's fill
    /// (`tab.activeBackground`) is the same value as the editor beneath
    /// it, so the two read as one continuous surface.
    fn editor_group(&self) -> El {
        let tabs: Vec<(String, String)> =
            self.open.iter().map(|t| (t.clone(), t.clone())).collect();

        column([
            editor_tabs(TABS_KEY, &self.active, tabs)
                .fill(vs::EDITOR_GROUP_HEADER_TABS_BG)
                .border_b()
                .border_color(vs::EDITOR_GROUP_HEADER_TABS_BORDER)
                .width(Size::Fill(1.0)),
            self.editor_body(),
        ])
        .height(Size::Fill(1.0))
        .width(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    fn editor_body(&self) -> El {
        column([
            row([
                text(&self.active).label().text_color(vs::TAB_ACTIVE_FG),
                chip("modified"),
                spacer(),
                button("Run").key("run").primary(),
                button("Debug").key("debug").secondary(),
            ])
            .gap(tokens::SPACE_2)
            .width(Size::Fill(1.0))
            .align(Align::Center),
            hairline(),
            column([
                text("// The four inversions, visible in one screen:")
                    .mono()
                    .caption(),
                text("//   1. controls on the 28px Xs rung")
                    .mono()
                    .caption(),
                text("//   2. corners at ~2px, controls included")
                    .mono()
                    .caption(),
                text("//   3. side bar and bars step above the content well")
                    .mono()
                    .caption(),
                text("//   4. regions meet on 1px borders, not gaps")
                    .mono()
                    .caption(),
            ])
            .gap(tokens::SPACE_1)
            .width(Size::Fill(1.0)),
            spacer(),
            // A stock card. Under this theme its fill is the same value
            // the chrome is painted with (`sideBar.background` names it),
            // so it reads as a docked panel rather than a sheet floating
            // on the canvas.
            card([
                card_header([card_title("Panel, not card")]),
                card_content([text(
                    "Same stock `card()` as anywhere else. The `card` slot \
                     is what `sideBar.background` names, one value step \
                     above the content well — so containment reads as a \
                     level, not as an outline.",
                )
                .caption()]),
            ])
            .width(Size::Fill(1.0)),
        ])
        .gap(tokens::SPACE_3)
        .padding(tokens::SPACE_3)
        .fill(vs::EDITOR_BG)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
    }
}

impl App for Shell {
    fn build(&self, _cx: &BuildCx) -> El {
        // A bare column, not `page()`: a workbench shell is full-bleed
        // by definition — window padding would float the whole
        // instrument on a margin, which is signal 4 of the diagnosis
        // this crate exists to invert.
        //
        // Full-bleed does not mean overlay-free, though. The shell
        // carries a tooltip, and the runtime pushes the tooltip layer
        // in as the root's last child — which floats only if the root
        // stacks its children. So the root has to be an
        // `Axis::Overlay` container; under this bare column the layer
        // would become a third row and squash the status bar.
        // `overlays(shell, [])` at the bottom is the whole cost, and it
        // is layout-neutral: `shell` still fills the viewport. Debug
        // builds assert on the first hover, and the bundle lint reports
        // `TooltipWithoutOverlayRoot` before that ever happens.
        let shell = column([
            self.title_strip(),
            row([self.side_bar(), self.editor_group()])
                .height(Size::Fill(1.0))
                .width(Size::Fill(1.0))
                .align(Align::Stretch),
            status_bar(
                [
                    // The one tooltip in this shell. It needs a
                    // `.key(...)` on this exact node — hit-test only
                    // returns keyed nodes, so an unkeyed tooltip is
                    // silently dead (lint: `DeadTooltip`).
                    text("main*")
                        .caption()
                        .key("scm")
                        .tooltip("Branch: main (modified)"),
                    text("0 problems").caption(),
                    chip(format!("{}", self.files.len())),
                ],
                [
                    text("Ln 1, Col 1").caption(),
                    text("UTF-8").caption(),
                    text("Rust").caption(),
                ],
            ),
        ])
        .align(Align::Stretch)
        .fill(vs::EDITOR_BG);

        overlays(shell, [])
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if editor_tabs::apply_event(
            &mut self.open,
            &mut self.active,
            &event,
            TABS_KEY,
            |raw| Some(raw.to_string()),
            || "untitled".to_string(),
        ) {
            return;
        }
        for f in &self.files.clone() {
            if event.is_click_or_activate(&format!("file:{f}")) {
                self.active = (*f).to_string();
                if !self.open.iter().any(|t| t == f) {
                    self.open.push((*f).to_string());
                }
                return;
            }
        }
    }

    fn theme(&self) -> Theme {
        theme::theme()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 1040.0, 660.0);
    damascene_winit_wgpu::run("Damascene — workbench shell", viewport, Shell::new())
}
