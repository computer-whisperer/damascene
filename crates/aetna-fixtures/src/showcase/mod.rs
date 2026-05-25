//! Showcase — Aetna's hero demo.
//!
//! Pages across six groups: every shadcn-shaped widget gets a
//! demo on a category page, every system-level capability (theme swap,
//! hotkeys, animation, custom shaders, overlays, toasts) gets a page
//! that exercises it end-to-end. The sidebar's theme picker swaps the
//! active palette live so all pages can be browsed under any theme.

use aetna_core::prelude::*;

pub mod about;
pub mod animation;
pub mod booleans;
pub mod buttons;
pub mod color_management;
pub mod diagnostics;
pub mod forms;
pub mod hotkeys;
pub mod html;
pub mod layout;
pub mod lists_tables;
pub mod math;
pub mod media;
pub mod overlays;
pub mod page_chrome;
pub mod palette;
pub mod scene3d;
pub mod shell;
pub mod status;
pub mod surfaces;
pub mod tabs_accordion;
pub mod text_inputs;
pub mod theme_choice;
pub mod typography;

pub use shell::{DIAGNOSTICS_TOGGLE_KEY, SECTION_PICKER_KEY, THEME_PICKER_KEY};

/// Returns `true` when this frame's viewport is below the showcase's
/// phone breakpoint. Section views call this so they can collapse rows
/// to columns, hide secondary chrome, or wrap text differently without
/// re-importing the breakpoint constant from `shell`.
pub(crate) fn is_phone(cx: &BuildCx) -> bool {
    cx.viewport_below(shell::PHONE_BREAKPOINT_PX)
}
pub use theme_choice::ThemeChoice;

/// WGSL for the liquid-glass custom shader. Surfaced through
/// [`Showcase::shaders`] so host harnesses register it once at startup;
/// the shader source lives next to the fixture so the registration and
/// the consumer can never drift.
pub const LIQUID_GLASS_WGSL: &str = include_str!("../../shaders/liquid_glass.wgsl");

/// Which page is currently mounted in the content panel. Each variant
/// owns its own state struct on [`Showcase`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Section {
    /// Landing page — short framing of the project plus a small live
    /// teaser. Default on first launch so visitors hitting the wasm
    /// build land on something narrative rather than a swatch grid.
    #[default]
    About,
    /// Token swatches grid for the active theme — the hero shot.
    Palette,
    /// Inspect the host's color-management state: working color space,
    /// negotiated wire description, compositor capability matrix.
    ColorManagement,
    Typography,
    Math,
    Html,
    Surfaces,
    Layout,
    Buttons,
    Booleans,
    TextInputs,
    Forms,
    Status,
    Media,
    Scene3D,
    ListsTables,
    TabsAccordion,
    Overlays,
    PageChrome,
    Animation,
    Hotkeys,
}

/// Sidebar grouping. Each section belongs to exactly one group.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Welcome,
    Theme,
    Foundations,
    Inputs,
    Display,
    Navigation,
    Patterns,
}

impl Section {
    pub const ALL: [Section; 21] = [
        Section::About,
        Section::Palette,
        Section::ColorManagement,
        Section::Typography,
        Section::Math,
        Section::Html,
        Section::Surfaces,
        Section::Layout,
        Section::Buttons,
        Section::Booleans,
        Section::TextInputs,
        Section::Forms,
        Section::Status,
        Section::Media,
        Section::Scene3D,
        Section::ListsTables,
        Section::TabsAccordion,
        Section::Overlays,
        Section::PageChrome,
        Section::Animation,
        Section::Hotkeys,
    ];

    /// Sidebar label.
    pub fn label(self) -> &'static str {
        match self {
            Section::About => "About",
            Section::Palette => "Palette",
            Section::ColorManagement => "Color management",
            Section::Typography => "Typography",
            Section::Math => "Math",
            Section::Html => "HTML",
            Section::Surfaces => "Surfaces",
            Section::Layout => "Layout",
            Section::Buttons => "Buttons & toggles",
            Section::Booleans => "Booleans",
            Section::TextInputs => "Text & value",
            Section::Forms => "Forms",
            Section::Status => "Status & feedback",
            Section::Media => "Media",
            Section::Scene3D => "3D scene",
            Section::ListsTables => "Lists & tables",
            Section::TabsAccordion => "Tabs & accordion",
            Section::Overlays => "Overlays",
            Section::PageChrome => "Page chrome",
            Section::Animation => "Animation",
            Section::Hotkeys => "Hotkeys",
        }
    }

    /// Slug used in routed-key prefixes and bin output filenames.
    pub fn slug(self) -> &'static str {
        match self {
            Section::About => "about",
            Section::Palette => "palette",
            Section::ColorManagement => "color-management",
            Section::Typography => "typography",
            Section::Math => "math",
            Section::Html => "html",
            Section::Surfaces => "surfaces",
            Section::Layout => "layout",
            Section::Buttons => "buttons",
            Section::Booleans => "booleans",
            Section::TextInputs => "text-inputs",
            Section::Forms => "forms",
            Section::Status => "status",
            Section::Media => "media",
            Section::Scene3D => "scene3d",
            Section::ListsTables => "lists-tables",
            Section::TabsAccordion => "tabs-accordion",
            Section::Overlays => "overlays",
            Section::PageChrome => "page-chrome",
            Section::Animation => "animation",
            Section::Hotkeys => "hotkeys",
        }
    }

    /// Sidebar grouping this section belongs to.
    pub fn group(self) -> Group {
        match self {
            Section::About => Group::Welcome,
            Section::Palette | Section::ColorManagement => Group::Theme,
            Section::Typography
            | Section::Math
            | Section::Html
            | Section::Surfaces
            | Section::Layout => Group::Foundations,
            Section::Buttons | Section::Booleans | Section::TextInputs | Section::Forms => {
                Group::Inputs
            }
            Section::Status | Section::Media | Section::Scene3D | Section::ListsTables => {
                Group::Display
            }
            Section::TabsAccordion | Section::Overlays | Section::PageChrome => Group::Navigation,
            Section::Animation | Section::Hotkeys => Group::Patterns,
        }
    }

    /// Sidebar nav key — a click on the matching button switches sections.
    pub fn nav_key(self) -> String {
        format!("nav-{}", self.slug())
    }

    /// Inverse of [`Self::slug`]. Used by the phone topbar's section
    /// picker to map the slug emitted by `select_menu` back to a
    /// `Section`.
    pub fn from_slug(slug: &str) -> Option<Section> {
        Section::ALL.into_iter().find(|s| s.slug() == slug)
    }
}

impl Group {
    pub const ALL: [Group; 7] = [
        Group::Welcome,
        Group::Theme,
        Group::Foundations,
        Group::Inputs,
        Group::Display,
        Group::Navigation,
        Group::Patterns,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::Welcome => "Welcome",
            Group::Theme => "Theme",
            Group::Foundations => "Foundations",
            Group::Inputs => "Inputs",
            Group::Display => "Display",
            Group::Navigation => "Navigation",
            Group::Patterns => "Patterns",
        }
    }

    /// Sections in this group, in sidebar order.
    pub fn sections(self) -> Vec<Section> {
        Section::ALL
            .iter()
            .copied()
            .filter(|s| s.group() == self)
            .collect()
    }
}

/// The showcase app. Per-section state persists across switches so
/// jumping in and out of a page is non-destructive. Fields are
/// `pub(crate)` so per-page layer-factory helpers (in their own
/// modules) can read both the active section and the relevant page
/// state to decide whether to mount their floating layer.
#[derive(Default)]
pub struct Showcase {
    pub(crate) section: Section,
    pub(crate) theme_choice: ThemeChoice,
    pub(crate) theme_picker_open: bool,
    pub(crate) theme_picker_density: MenuDensity,
    /// Open state for the phone topbar's section dropdown. Mirrors
    /// `theme_picker_open`; only consulted when the shell renders the
    /// phone layout, but the field exists at all viewport sizes so
    /// switching across the breakpoint mid-frame doesn't drop state.
    pub(crate) section_picker_open: bool,
    pub(crate) section_picker_density: MenuDensity,
    /// When true, mount the host-diagnostics overlay. Defaults to false
    /// so the panel doesn't sit on top of overlay/page content unless
    /// the user opts in via the sidebar toggle.
    pub(crate) diagnostics_visible: bool,

    pub(crate) about: about::State,
    pub(crate) typography: typography::State,
    pub(crate) html: html::State,
    /// URLs accumulated from `UiEventKind::LinkActivated` events.
    /// Drained by the host once per frame via [`App::drain_link_opens`]
    /// — the host owns the platform-appropriate open call. Showcase
    /// keeps the field private; the only producer is the Activate arm
    /// in `on_event`, the only consumer is the trait impl.
    pub(crate) pending_link_opens: Vec<String>,
    pub(crate) surfaces: surfaces::State,
    pub(crate) layout: layout::State,
    pub(crate) math: math::State,
    pub(crate) buttons: buttons::State,
    pub(crate) booleans: booleans::State,
    pub(crate) text_inputs: text_inputs::State,
    pub(crate) forms: forms::State,
    pub(crate) status: status::State,
    pub(crate) scene3d: scene3d::State,
    pub(crate) lists_tables: lists_tables::State,
    pub(crate) tabs_accordion: tabs_accordion::State,
    pub(crate) overlays: overlays::State,
    pub(crate) page_chrome: page_chrome::State,
    pub(crate) animation: animation::State,
    pub(crate) hotkeys: hotkeys::State,

    /// Optional app-owned GPU texture surfaced on the Media page to
    /// demonstrate `surface()`. The fixtures crate is backend-neutral,
    /// so it can't allocate one itself — the host wires it up (see
    /// `examples/src/bin/showcase.rs` for the wgpu side that
    /// allocates a `wgpu::Texture` in `gpu_setup`, and
    /// `aetna-ash-demo` for the ash host-owned image path).
    pub(crate) animated_surface: Option<aetna_core::surface::AppTexture>,
}

impl Showcase {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct with a specific starting page. Used by headless render
    /// bins to pin the showcase on one section without driving the
    /// navigation through events.
    pub fn with_section(section: Section) -> Self {
        Self {
            section,
            ..Default::default()
        }
    }

    /// Construct the Overlays page with the stock dropdown menu open.
    /// Used by generated fixture bundles so stock floating menus are
    /// covered by local lint artifacts.
    pub fn with_overlay_dropdown_open() -> Self {
        let mut app = Self::with_section(Section::Overlays);
        app.overlays.dropdown_open = true;
        app
    }

    /// Construct the Overlays page with the stock context menu open at
    /// a viewport point. Used by generated fixture bundles so dense
    /// menu rows exercise focus-ring clipping/obscuring lint locally.
    pub fn with_overlay_context_menu_at(x: f32, y: f32) -> Self {
        let mut app = Self::with_section(Section::Overlays);
        app.overlays.context_menu_at = Some((x, y));
        app
    }

    /// Hand the showcase an app-owned GPU texture for the Media page's
    /// `surface()` demo. Pass `None` to clear it (the page falls back
    /// to a placeholder explaining the demo needs a wgpu host).
    pub fn set_animated_surface(&mut self, tex: Option<aetna_core::surface::AppTexture>) {
        self.animated_surface = tex;
    }

    /// Borrow the registered animated-surface texture, if any.
    pub fn animated_surface(&self) -> Option<&aetna_core::surface::AppTexture> {
        self.animated_surface.as_ref()
    }
}

impl App for Showcase {
    fn build(&self, cx: &BuildCx) -> El {
        let theme = self.theme();
        let body = match self.section {
            Section::About => about::view(&self.about, cx),
            Section::Palette => palette::view(theme.palette(), cx),
            Section::ColorManagement => color_management::view(cx),
            Section::Typography => typography::view(&self.typography),
            Section::Math => math::view(&self.math, cx),
            Section::Html => html::view(&self.html, cx),
            Section::Surfaces => surfaces::view(&self.surfaces, cx),
            Section::Layout => layout::view(&self.layout, cx),
            Section::Buttons => buttons::view(&self.buttons, cx),
            Section::Booleans => booleans::view(&self.booleans),
            Section::TextInputs => text_inputs::view(&self.text_inputs, cx),
            Section::Forms => forms::view(&self.forms),
            Section::Status => status::view(&self.status, cx),
            Section::Media => media::view(self.animated_surface.as_ref(), cx),
            Section::Scene3D => scene3d::view(&self.scene3d),
            Section::ListsTables => lists_tables::view(&self.lists_tables, cx),
            Section::TabsAccordion => tabs_accordion::view(&self.tabs_accordion, cx),
            Section::Overlays => overlays::view(&self.overlays),
            Section::PageChrome => page_chrome::view(&self.page_chrome, cx),
            Section::Animation => animation::view(&self.animation),
            Section::Hotkeys => hotkeys::view(&self.hotkeys),
        };
        let (main, mut layers) = shell::frame(self, cx, body);
        // Mount the diagnostic overlay on top of every page when the
        // host attached a `HostDiagnostics` *and* the sidebar toggle is
        // on. Hosts that opt out (the headless render bins,
        // vulkano-demo, anything that doesn't call
        // `BuildCx::with_diagnostics`) get the showcase exactly as
        // before — no overlay, no extra widgets in the tree.
        if self.diagnostics_visible
            && let Some(diag) = cx.diagnostics()
        {
            layers.push(Some(diagnostics::layer(diag)));
        }
        overlay_root(main, layers)
    }

    fn hotkeys(&self) -> Vec<(KeyChord, String)> {
        match self.section {
            Section::Hotkeys => hotkeys::hotkeys(&self.hotkeys),
            _ => Vec::new(),
        }
    }

    fn theme(&self) -> Theme {
        self.theme_choice.theme()
    }

    fn on_event(&mut self, event: UiEvent) {
        // Link click — accumulate the URL for the host to open. The
        // showcase doesn't filter or transform; in a real app this is
        // the spot to short-circuit links to internal routes, prompt
        // for confirmation on outbound URLs, etc.
        if event.kind == UiEventKind::LinkActivated
            && let Some(url) = event.route()
        {
            self.pending_link_opens.push(url.to_string());
            return;
        }

        // Sidebar nav — any click on a `nav-*` key switches section.
        if matches!(event.kind, UiEventKind::Click | UiEventKind::Activate)
            && let Some(k) = event.route()
            && let Some(target) = nav_section(k)
        {
            self.section = target;
            return;
        }

        // Sidebar diagnostics toggle.
        if switch::apply_event(
            &mut self.diagnostics_visible,
            &event,
            DIAGNOSTICS_TOGGLE_KEY,
        ) {
            return;
        }

        // Theme picker (sidebar select dropdown).
        let mut token = self.theme_choice.token().to_string();
        if select::apply_event(
            &mut token,
            &mut self.theme_picker_open,
            &event,
            THEME_PICKER_KEY,
            |s| ThemeChoice::from_token(&s).map(|c| c.token().to_string()),
        ) {
            if event.is_click_or_activate(THEME_PICKER_KEY) {
                self.theme_picker_density = if self.theme_picker_open {
                    MenuDensity::from_event(&event)
                } else {
                    MenuDensity::Compact
                };
            } else if !self.theme_picker_open {
                self.theme_picker_density = MenuDensity::Compact;
            }
            if let Some(choice) = ThemeChoice::from_token(&token) {
                self.theme_choice = choice;
            }
            return;
        }

        // Phone topbar section picker. Only mounted on narrow viewports,
        // but the handler runs unconditionally — picking via keyboard or
        // a stale layer should still route correctly.
        let mut slug = self.section.slug().to_string();
        if select::apply_event(
            &mut slug,
            &mut self.section_picker_open,
            &event,
            SECTION_PICKER_KEY,
            |s| Section::from_slug(&s).map(|sec| sec.slug().to_string()),
        ) {
            if event.is_click_or_activate(SECTION_PICKER_KEY) {
                self.section_picker_density = if self.section_picker_open {
                    MenuDensity::from_event(&event)
                } else {
                    MenuDensity::Compact
                };
            } else if !self.section_picker_open {
                self.section_picker_density = MenuDensity::Compact;
            }
            if let Some(section) = Section::from_slug(&slug) {
                self.section = section;
            }
            return;
        }

        match self.section {
            Section::About => about::on_event(&mut self.about, event),
            Section::Palette => {}         // static — no events
            Section::ColorManagement => {} // read-only, reflects host state
            Section::Typography => typography::on_event(&mut self.typography, event),
            Section::Math => math::on_event(&mut self.math, event),
            Section::Html => html::on_event(&mut self.html, event),
            Section::Surfaces => surfaces::on_event(&mut self.surfaces, event),
            Section::Layout => layout::on_event(&mut self.layout, event),
            Section::Buttons => buttons::on_event(&mut self.buttons, event),
            Section::Booleans => booleans::on_event(&mut self.booleans, event),
            Section::TextInputs => text_inputs::on_event(&mut self.text_inputs, event),
            Section::Forms => forms::on_event(&mut self.forms, event),
            Section::Status => status::on_event(&mut self.status, event),
            Section::Media => {} // static
            Section::Scene3D => scene3d::on_event(&mut self.scene3d, event),
            Section::ListsTables => lists_tables::on_event(&mut self.lists_tables, event),
            Section::TabsAccordion => tabs_accordion::on_event(&mut self.tabs_accordion, event),
            Section::Overlays => overlays::on_event(&mut self.overlays, event),
            Section::PageChrome => page_chrome::on_event(&mut self.page_chrome, event),
            Section::Animation => animation::on_event(&mut self.animation, event),
            Section::Hotkeys => hotkeys::on_event(&mut self.hotkeys, event),
        }
    }

    fn drain_toasts(&mut self) -> Vec<ToastSpec> {
        let mut toasts = std::mem::take(&mut self.status.pending_toasts);
        toasts.append(&mut self.about.pending_toasts);
        toasts
    }

    fn drain_scroll_requests(&mut self) -> Vec<aetna_core::scroll::ScrollRequest> {
        match self.section {
            Section::Math => math::drain_scroll_requests(&mut self.math),
            Section::Html => html::drain_scroll_requests(&mut self.html),
            Section::TextInputs => text_inputs::drain_scroll_requests(&mut self.text_inputs),
            Section::Forms => forms::drain_scroll_requests(&mut self.forms),
            _ => Vec::new(),
        }
    }

    fn selection(&self) -> Selection {
        // Surface whichever section currently owns a focused text
        // input — the runtime uses it to paint highlight bands and
        // resolve clipboard ops. The About page has its own dispatcher;
        // sections that host inputs return their own selection here.
        match self.section {
            Section::About => self.about.message_selection.clone(),
            Section::Typography => self.typography.selection.clone(),
            Section::TextInputs => self.text_inputs.selection.clone(),
            Section::Forms => self.forms.selection.clone(),
            Section::Math => self.math.selection.clone(),
            Section::Html => self.html.selection.clone(),
            _ => Selection::default(),
        }
    }

    fn drain_link_opens(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_link_opens)
    }

    fn shaders(&self) -> Vec<AppShader> {
        // The Surfaces page mounts the liquid_glass card. Register the
        // custom shader so the runtime has it at paint time regardless
        // of which section is currently shown.
        vec![AppShader {
            name: "liquid_glass",
            wgsl: LIQUID_GLASS_WGSL,
            samples_backdrop: true,
            samples_time: false,
        }]
    }
}

fn nav_section(key: &str) -> Option<Section> {
    Section::ALL.into_iter().find(|s| s.nav_key() == key)
}

/// Wrap the main view + floating layers in an overlay root. Layers are
/// only present when their controlling open-flag is set, otherwise they
/// drop out of the tree entirely.
fn overlay_root(main: El, layers: Vec<Option<El>>) -> El {
    overlays(main, layers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trigger_event(key: &str, pointer_kind: Option<PointerKind>) -> UiEvent {
        let mut event = UiEvent::synthetic_click(key);
        event.pointer_kind = pointer_kind;
        event
    }

    #[test]
    fn section_picker_records_touch_density_when_opened_by_touch() {
        let mut app = Showcase::default();

        app.on_event(trigger_event(SECTION_PICKER_KEY, Some(PointerKind::Touch)));

        assert!(app.section_picker_open);
        assert_eq!(app.section_picker_density, MenuDensity::Touch);
    }

    #[test]
    fn theme_picker_records_compact_density_when_opened_by_mouse() {
        let mut app = Showcase::default();

        app.on_event(trigger_event(THEME_PICKER_KEY, Some(PointerKind::Mouse)));

        assert!(app.theme_picker_open);
        assert_eq!(app.theme_picker_density, MenuDensity::Compact);
    }
}
