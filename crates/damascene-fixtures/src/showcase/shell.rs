//! Sidebar + content frame.
//!
//! The shell wraps every page in a sidebar / content row plus the set
//! of floating layers each page contributes. The sidebar uses the
//! proper `sidebar_*` widgets — `sidebar_header`, `sidebar_group`,
//! `sidebar_group_label`, `sidebar_menu`, `sidebar_menu_button` — so
//! the panel chrome and group-of-buttons spacing come from one source.
//!
//! Below `PHONE_BREAKPOINT_PX` the side panel is replaced with a
//! top bar containing the section picker, theme picker, and the
//! diagnostics toggle. The content area then takes the full viewport
//! width with reduced padding so phone displays don't waste space on
//! chrome.

use damascene_core::prelude::*;

use super::{Group, Section, Showcase, theme_choice::ThemeChoice};

pub const THEME_PICKER_KEY: &str = "theme-picker";
pub const SECTION_PICKER_KEY: &str = "section-picker";
pub const DIAGNOSTICS_TOGGLE_KEY: &str = "diagnostics-toggle";

/// Viewport width below which the shell drops the side panel and
/// renders the phone topbar instead. Picked to fit a 360px phone while
/// keeping the sidebar visible on tablets in portrait. `pub(super)` so
/// individual section views (`text_inputs`, etc.) can use the same
/// breakpoint to switch their per-page layouts.
pub(super) const PHONE_BREAKPOINT_PX: f32 = 700.0;

/// Build the main view (sidebar + content) plus the list of floating
/// layers each page contributes. `Showcase::build` wraps the result in
/// `overlays(main, layers)`.
pub fn frame(app: &Showcase, cx: &BuildCx, body: El) -> (El, Vec<Option<El>>) {
    let phone = cx.viewport_below(PHONE_BREAKPOINT_PX);
    let safe = cx.safe_area();
    let main = if phone {
        column([phone_topbar(app, safe.top), content(body, true, safe)])
    } else {
        row([sidebar_chrome(app, safe), content(body, false, safe)])
    };
    // Note: we deliberately do *not* apply `cx.safe_area_bottom()` as
    // padding at the shell root. The first attempt did
    // (`main.pb(safe_bottom)`) and it regressed soft-keyboard
    // behavior on phone: shrinking the column from the bottom
    // shrinks the inner scroll viewport, which can push a near-
    // bottom focused text-input outside the scroll's clip rect; the
    // focus pass excludes nodes whose rect falls outside their clip,
    // `sync_focus_order` then clears focus, and the host dismisses
    // the soft keyboard a frame later. The right fix is for each
    // page to scroll its focused input above the inset (or apply
    // padding inside the scroll, not outside it). The showcase just
    // exposes `cx.safe_area_bottom()` and leaves the application
    // layer as the consumer.
    // Each page contributes zero or one floating layer; theme picker is
    // always available across pages. `Showcase::build` wraps the result
    // in `overlays(main, layers)` and inactive layers drop out.
    let layers = vec![
        app.theme_picker_open
            .then(|| theme_picker_menu(app.theme_picker_density)),
        // Section picker only relevant on phone, but mounting the menu
        // when its open flag is set is unconditional — the picker
        // can't be opened when its trigger isn't in the tree, so this
        // is correct on desktop too.
        app.section_picker_open
            .then(|| section_picker_menu(app.section_picker_density)),
        super::overlays::dialog_layer(app),
        super::overlays::sheet_layer(app),
        super::overlays::popover_layer(app),
        super::overlays::context_menu_layer(app),
        super::overlays::dropdown_layer(app),
        super::text_inputs::region_layer(app),
        super::text_inputs::command_layer(app),
        super::tabs_accordion::actions_layer(app),
        (app.section == Section::PageChrome)
            .then(|| super::page_chrome::layer(&app.page_chrome))
            .flatten(),
    ];
    (main, layers)
}

fn sidebar_chrome(app: &Showcase, safe: Sides) -> El {
    let groups = Group::ALL
        .iter()
        .copied()
        .map(|g| group_block(app.section, g));

    sidebar([
        sidebar_header([
            text("Damascene").bold().font_size(18.0),
            text("showcase").muted().small(),
        ]),
        theme_picker(app.theme_choice),
        // Wrap the nav groups in a column with focus-ring slack plus a
        // right-side scrollbar gutter. Putting the padding *inside* the
        // scroll keeps the thumb at the scroll's right edge while
        // pulling the focusable buttons inward, so their focus rings
        // are not scissored and the thumb cannot cover them.
        scroll([column(groups.collect::<Vec<_>>())
            .gap(tokens::SPACE_3)
            .padding(Sides {
                left: tokens::RING_WIDTH,
                right: tokens::SCROLLBAR_HITBOX_WIDTH,
                top: tokens::RING_WIDTH,
                bottom: tokens::RING_WIDTH,
            })])
        .height(Size::Fill(1.0)),
        diagnostics_toggle(app.diagnostics_visible),
    ])
    .padding(Sides {
        left: tokens::SPACE_4 + safe.left,
        right: tokens::SPACE_4,
        top: tokens::SPACE_4 + safe.top,
        bottom: tokens::SPACE_4,
    })
}

fn group_block(active: Section, group: Group) -> El {
    let buttons: Vec<El> = group
        .sections()
        .into_iter()
        .map(|s| sidebar_menu_button(s.label(), s == active).key(s.nav_key()))
        .collect();
    sidebar_group([sidebar_group_label(group.label()), sidebar_menu(buttons)])
}

fn theme_picker(active: ThemeChoice) -> El {
    column([
        text("Theme").caption().muted(),
        select_trigger(THEME_PICKER_KEY, active.label()),
    ])
    .gap(tokens::SPACE_1)
    .padding(Sides::xy(0.0, tokens::SPACE_2))
}

/// Footer row that toggles the host-diagnostics overlay. Sits below the
/// nav scroll so it stays pinned to the bottom of the sidebar even when
/// the section list is long enough to scroll.
fn diagnostics_toggle(active: bool) -> El {
    row([
        text("Debug overlay").label().width(Size::Fill(1.0)),
        switch(active).key(DIAGNOSTICS_TOGGLE_KEY),
    ])
    .gap(tokens::SPACE_3)
    .padding(Sides::xy(0.0, tokens::SPACE_2))
    .align(Align::Center)
}

fn theme_picker_menu(density: MenuDensity) -> El {
    let options = ThemeChoice::ALL
        .iter()
        .map(|c| (c.token(), c.label()))
        .collect::<Vec<_>>();
    select_menu_with_density(THEME_PICKER_KEY, options, density)
}

/// Phone-only top bar: section picker (the wide column on the left so
/// nav remains the primary affordance), compact theme picker, and the
/// diagnostics toggle. Sits inside a `Card`-equivalent panel with a
/// bottom border so it reads as page chrome rather than content.
fn phone_topbar(app: &Showcase, safe_top: f32) -> El {
    row([
        // Section picker — fills the remaining width so the active
        // page's name is readable even on a 360px-wide screen.
        section_picker(app.section).width(Size::Fill(1.0)),
        // Theme picker — narrow, just shows the active theme name.
        section_chrome_theme(app.theme_choice),
        diagnostics_toggle_compact(app.diagnostics_visible),
    ])
    .style_profile(StyleProfile::Surface)
    .surface_role(SurfaceRole::Panel)
    .fill(tokens::CARD)
    .stroke(tokens::BORDER)
    .padding(Sides {
        left: tokens::SPACE_3,
        right: tokens::SPACE_3,
        top: tokens::SPACE_3 + safe_top,
        bottom: tokens::SPACE_3,
    })
    .gap(tokens::SPACE_2)
    .align(Align::Center)
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

fn section_picker(active: Section) -> El {
    select_trigger(SECTION_PICKER_KEY, active.label())
}

fn section_chrome_theme(active: ThemeChoice) -> El {
    // No "Theme" caption above — saves vertical space in the topbar.
    select_trigger(THEME_PICKER_KEY, active.label()).max_width(160.0)
}

/// Compact diagnostics toggle for the phone topbar — just the switch
/// without the label text, so it doesn't crowd out the dropdowns.
fn diagnostics_toggle_compact(active: bool) -> El {
    switch(active).key(DIAGNOSTICS_TOGGLE_KEY)
}

fn section_picker_menu(density: MenuDensity) -> El {
    // Group sections by their parent Group so the dropdown reads like
    // the sidebar nav. `select_menu` doesn't natively render group
    // headers, so we just flatten in nav order — same order the
    // sidebar shows.
    let options: Vec<(String, &'static str)> = Section::ALL
        .iter()
        .map(|s| (s.slug().to_string(), s.label()))
        .collect();
    select_menu_with_density(SECTION_PICKER_KEY, options, density)
}

/// Content panel. Padding shrinks on phone so the page body has
/// breathing room without wasting screen real estate on margins.
fn content(body: El, phone: bool, safe: Sides) -> El {
    let pad = if phone {
        tokens::SPACE_3
    } else {
        tokens::SPACE_7
    };
    column([body])
        .padding(Sides {
            left: pad + safe.left,
            right: pad + safe.right,
            top: pad + if phone { 0.0 } else { safe.top },
            bottom: pad,
        })
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect_menu_item_heights(el: &El, heights: &mut Vec<Size>) {
        if matches!(el.kind, Kind::Custom("menu_item")) {
            heights.push(el.height);
        }
        for child in &el.children {
            collect_menu_item_heights(child, heights);
        }
    }

    #[test]
    fn section_picker_menu_uses_touch_density() {
        let menu = section_picker_menu(MenuDensity::Touch);
        let mut heights = Vec::new();

        collect_menu_item_heights(&menu, &mut heights);

        assert_eq!(
            heights,
            vec![Size::Fixed(TOUCH_MENU_ITEM_HEIGHT); Section::ALL.len()]
        );
    }

    #[test]
    fn theme_picker_menu_keeps_compact_density_by_default() {
        let menu = theme_picker_menu(MenuDensity::Compact);
        let mut heights = Vec::new();

        collect_menu_item_heights(&menu, &mut heights);

        assert_eq!(heights, vec![Size::Fixed(28.0); ThemeChoice::ALL.len()]);
    }
}
