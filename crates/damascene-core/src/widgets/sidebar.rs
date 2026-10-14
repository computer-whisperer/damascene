//! Sidebar anatomy — familiar navigation groups and menu rows.
//!
//! `sidebar([...])` is the panel-surface wrapper: it bundles the
//! canonical [`SurfaceRole::Panel`] + `tokens::CARD` fill +
//! `tokens::BORDER` stroke + `tokens::SIDEBAR_WIDTH` width recipe.
//! [`sidebar_with`] adds the app-owned collapsed (icon-rail) mode.
//! The inner slots mirror shadcn's sidebar anatomy: [`sidebar_header`]
//! and [`sidebar_footer`] are the pinned top/bottom strips,
//! [`sidebar_content`] the filling middle between them,
//! [`sidebar_group`] a titled section ([`sidebar_group_label`] +
//! optional [`sidebar_group_action`]), [`sidebar_menu`] the nav list,
//! and [`sidebar_menu_button`] / [`sidebar_menu_button_with`] the rows.
//!
//! ```ignore
//! sidebar_with(
//!     [
//!         sidebar_header([row([icon("audio-lines"), text("Northgate Ops").title()])]),
//!         sidebar_content([sidebar_group([
//!             row([sidebar_group_label("Channels"), sidebar_group_action("plus").key("add")]),
//!             sidebar_menu([
//!                 sidebar_menu_button_with(
//!                     "War Room",
//!                     true,
//!                     SidebarMenuButtonOpts::default().icon("volume-2").trailing(badge("7")),
//!                 )
//!                 .key("ch:war-room"),
//!             ]),
//!         ])]),
//!         sidebar_footer([/* self strip */]),
//!     ],
//!     SidebarOpts::default().collapsed(self.rail_collapsed),
//! )
//! ```
//!
//! When your sidebar has shapes the helpers don't cover (collapsible
//! sections, nested sub-groups, custom row anatomy), **wrap your custom
//! composition in `sidebar([...])` and skip the inner helpers** — that
//! keeps the canonical surface recipe correct without forcing your row
//! data into the helper mold.
//!
//! # Oracle (`docs/NAMING_ORACLE.md`)
//!
//! shadcn/ui `Sidebar` — registry JSON fetched 2026-07-26 from
//! `https://ui.shadcn.com/r/styles/new-york/sidebar.json`. Slot names
//! map 1:1 (snake-cased): `SidebarHeader`/`SidebarFooter`/
//! `SidebarContent`/`SidebarGroup`/`SidebarGroupLabel`/
//! `SidebarGroupAction`/`SidebarMenu`/`SidebarMenuItem`/
//! `SidebarMenuButton`. Width tokens match: `--sidebar-width` 16rem →
//! [`tokens::SIDEBAR_WIDTH`] 256, `--sidebar-width-icon` 3rem →
//! [`tokens::SIDEBAR_WIDTH_ICON`] 48, and the collapsed menu button's
//! `!size-8` square → [`tokens::CONTROL_HEIGHT`] 32.
//!
//! Recorded divergences from the oracle:
//!
//! - **Collapsed mode is a rebuild, not CSS.** shadcn's `collapsible`
//!   prop offers `offcanvas` (slide away) and `icon` (rail) modes,
//!   driven by a `SidebarProvider` context, animated over 200ms, with a
//!   floating `Sheet` on mobile. Damascene v1 ships the `icon` mode
//!   only, from an app-owned `collapsed: bool` on [`SidebarOpts`] —
//!   the app flips its own state and rebuilds, per the library-wide
//!   state philosophy. **No offcanvas mode** (a workbench rail
//!   collapses to icons rather than vanishing; hiding the pane is the
//!   shell's split-layout decision, not the sidebar's) and **no
//!   width animation v1** (layout-size animation is a separate arc;
//!   the rebuild snaps). No `SidebarProvider`/`useSidebar`/
//!   `SidebarTrigger`/`SidebarRail` either — a trigger is any keyed
//!   `icon_button("panel-left")` the app wires to its flag.
//! - **Trailing content flows; shadcn's is absolutely positioned.**
//!   `SidebarMenuBadge`/`SidebarMenuAction` float over the row's right
//!   edge; damascene has no absolute positioning, so
//!   [`SidebarMenuButtonOpts::trailing`] is a flex slot after the
//!   filling label — the same shape as `TreeItemOpts::trailing`.
//!   Likewise [`sidebar_group_action`] composes in a `row` with the
//!   label instead of floating at the group's top-right corner.
//! - **Slot padding lives on the rail.** shadcn pads every slot `p-2`
//!   on an unpadded rail; damascene pads the rail once
//!   ([`tokens::SPACE_4`] expanded, [`tokens::SPACE_2`] collapsed) and
//!   the slots are flush fill-width columns.
//! - **[`sidebar_content`] does not scroll by default** (shadcn:
//!   `overflow-auto`). Chain `.scrollable()` or wrap the groups in
//!   `scroll([...])` when the nav outgrows the rail — scrolling wants
//!   a `.key(...)` and damascene keeps that decision visible.
//! - Not shipped (use the stock widget instead, or deferred):
//!   `SidebarSeparator` (= `separator()`), `SidebarInput`
//!   (= `text_input`), `SidebarInset` (the shell's own main-pane
//!   recipe), `SidebarMenuSkeleton` (= `skeleton()`),
//!   `SidebarMenuSub*` (nested nav is `tree()`'s job).

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::a11y::Role;
use crate::anim::Timing;
use crate::cursor::Cursor;
use crate::icons::svg::IconSource;
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::text;
use crate::{IntoIconSource, icon};

/// Square size of a [`sidebar_group_action`] button — shadcn's `w-5`
/// (20 px) group-action square.
pub const SIDEBAR_GROUP_ACTION_SIZE: f32 = 20.0;

/// Options for [`sidebar_with`] — the app-owned display mode of the
/// rail.
#[derive(Clone, Debug, Default)]
pub struct SidebarOpts {
    /// Render the collapsed icon rail (shadcn `collapsible="icon"`,
    /// `data-state="collapsed"`): a [`tokens::SIDEBAR_WIDTH_ICON`]-wide
    /// column where menu buttons shrink to icon-only squares and all
    /// label-tier content (text, headings, badges, group labels, group
    /// actions) is dropped. The app owns this flag — wire a keyed
    /// trigger (e.g. `icon_button("panel-left").key("rail")`) and flip
    /// it in `on_event`.
    pub collapsed: bool,
}

impl SidebarOpts {
    /// Set [`SidebarOpts::collapsed`].
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }
}

/// Navigation rail surface — a [`tokens::SIDEBAR_WIDTH`]-wide, full-height
/// panel column with the canonical card fill + border recipe. Wrap custom
/// nav compositions in this even when skipping the inner helpers.
///
/// [`sidebar_with`] with default [`SidebarOpts`]; see it for the
/// collapsed icon-rail mode.
#[track_caller]
pub fn sidebar<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    sidebar_with(children, SidebarOpts::default()).at_loc(Location::caller())
}

/// [`sidebar`] with options. `collapsed: false` is exactly
/// [`sidebar`]; `collapsed: true` renders the icon rail — fixed
/// [`tokens::SIDEBAR_WIDTH_ICON`] width, and the same children
/// transformed for icon-only display:
///
/// - menu buttons keep only their icons and shrink to
///   [`tokens::CONTROL_HEIGHT`] squares (shadcn `!size-8`), keeping
///   their key, focus, and `current` treatment. A button built without
///   an icon keeps its blank square — the oracle clips the label out of
///   an identical square rather than hiding the button, so the click
///   target and current-page marker survive; give rail-visible rows
///   icons.
/// - text, headings, badges, [`sidebar_group_label`]s, and
///   [`sidebar_group_action`]s are dropped (shadcn hides each via
///   `group-data-[collapsible=icon]`), which is what collapses the
///   header/footer strips down to their icons.
/// - plain row containers restack vertically, centered — a 32 px
///   content column has no horizontal room, so a footer's
///   avatar + mic + settings cluster becomes a rail stack (the
///   structural stand-in for the oracle's `overflow-hidden`
///   clipping).
///
/// The transform is structural, so it applies equally to the stock
/// slots and to custom compositions wrapped in the rail.
#[track_caller]
pub fn sidebar_with<I, E>(children: I, opts: SidebarOpts) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    if !opts.collapsed {
        return column(children)
            .at_loc(Location::caller())
            .style_profile(StyleProfile::Surface)
            .surface_role(SurfaceRole::Panel)
            .fill(tokens::CARD)
            .stroke(tokens::BORDER)
            .width(Size::Fixed(tokens::SIDEBAR_WIDTH))
            .height(Size::Fill(1.0))
            .default_padding(tokens::SPACE_4)
            .default_gap(tokens::SPACE_4);
    }

    let collapsed: Vec<El> = children
        .into_iter()
        .map(Into::into)
        .filter_map(collapse_to_icon_rail)
        .collect();
    column(collapsed)
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .surface_role(SurfaceRole::Panel)
        .fill(tokens::CARD)
        .stroke(tokens::BORDER)
        .width(Size::Fixed(tokens::SIDEBAR_WIDTH_ICON))
        .height(Size::Fill(1.0))
        .align(Align::Center)
        .default_padding(tokens::SPACE_2)
        .default_gap(tokens::SPACE_4)
}

/// The collapsed-mode structural transform (see [`sidebar_with`]).
/// Returns `None` for label-tier content the icon rail drops.
fn collapse_to_icon_rail(mut el: El) -> Option<El> {
    match el.kind {
        // Label-tier content is hidden in the icon rail: shadcn drops
        // group labels via `-mt-8 opacity-0`, menu badges and group
        // actions via `group-data-[collapsible=icon]:hidden`, and
        // clips button labels behind `overflow-hidden !size-8`.
        Kind::Text | Kind::Heading | Kind::Badge => return None,
        Kind::Custom("sidebar_group_action") => return None,
        Kind::Custom("sidebar_menu_button") => {
            // Icon-only square (shadcn `!size-8 !p-2`): keep the icon,
            // drop label and trailing slot, keep key / focus / current.
            el.children.retain(|c| c.kind == Kind::Custom("icon"));
            return Some(
                el.width(Size::Fixed(tokens::CONTROL_HEIGHT))
                    .height(Size::Fixed(tokens::CONTROL_HEIGHT))
                    .padding(Sides::zero())
                    .justify(Justify::Center),
            );
        }
        _ => {}
    }
    let children = std::mem::take(&mut el.children);
    el.children = children
        .into_iter()
        .filter_map(collapse_to_icon_rail)
        .collect();
    // A 32 px content column has no horizontal room: plain row
    // containers (a header's logo + title cluster, a footer's
    // avatar + action strip) restack vertically, centered. This is
    // the structural translation of the oracle's `overflow-hidden`
    // clipping — instead of cropping the cluster at the rail edge,
    // the surviving icons stack down the rail.
    if el.kind == Kind::Group && el.axis == Axis::Row {
        el = el.axis(Axis::Column).align(Align::Center).width(Size::Fill(1.0));
    }
    Some(el)
}

/// Header slot pinned at the top of the rail (app name, workspace
/// switcher) — shadcn `SidebarHeader`. Collapsed mode strips its
/// text down to the icons.
#[track_caller]
pub fn sidebar_header<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .gap(tokens::SPACE_1)
}

/// Footer slot pinned at the bottom of the rail (the signed-in-user
/// strip, connection status) — shadcn `SidebarFooter`, mirror of
/// [`sidebar_header`]. It pins because [`sidebar_content`] (or a
/// `spacer()`) fills the slack above it.
#[track_caller]
pub fn sidebar_footer<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .gap(tokens::SPACE_1)
}

/// Filling middle of the rail between [`sidebar_header`] and
/// [`sidebar_footer`] — shadcn `SidebarContent` (`flex-1`), the slot
/// that makes the footer bottom-pinned. Stacks [`sidebar_group`]s at
/// the rail's [`tokens::SPACE_4`] rhythm. Not scrollable by default
/// (recorded divergence: shadcn is `overflow-auto`) — chain
/// `.scrollable()` (plus a `.key(...)`) when the nav outgrows the
/// rail.
#[track_caller]
pub fn sidebar_content<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .default_gap(tokens::SPACE_4)
}

/// A titled section of the rail — typically a [`sidebar_group_label`]
/// followed by a [`sidebar_menu`]. Shadcn `SidebarGroup`.
#[track_caller]
pub fn sidebar_group<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .gap(tokens::SPACE_1)
}

/// Non-interactive muted caption heading a [`sidebar_group`] — shadcn
/// `SidebarGroupLabel`. For a group action beside it, put both in a
/// row: `row([sidebar_group_label("Channels"),
/// sidebar_group_action("plus").key("add")]).align(Align::Center)` —
/// the label fills, so the action hugs the right edge.
#[track_caller]
pub fn sidebar_group_label(label: impl Into<String>) -> El {
    text(label)
        .at_loc(Location::caller())
        .caption()
        .semibold()
        .muted()
        .ellipsis()
        .padding(Sides {
            left: 0.0,
            right: tokens::SPACE_2,
            top: tokens::SPACE_1,
            bottom: 0.0,
        })
        .width(Size::Fill(1.0))
}

/// Small ghost icon button at a group heading's right edge (add a
/// channel, collapse the section) — shadcn `SidebarGroupAction`: a
/// `w-5` (= [`SIDEBAR_GROUP_ACTION_SIZE`]) square with a `size-4`
/// ([`tokens::ICON_SM`]) glyph in the sidebar's base foreground.
/// Composes in a row after [`sidebar_group_label`] (see there);
/// recorded divergence: shadcn floats it absolutely over the group's
/// top-right corner. Give it a `.key(...)` and a `.name(...)` — the
/// glyph is the whole control. Dropped in collapsed mode, as in the
/// oracle.
#[track_caller]
pub fn sidebar_group_action(source: impl IntoIconSource) -> El {
    El::new(Kind::Custom("sidebar_group_action"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Solid)
        .metrics_role(MetricsRole::IconButton)
        .focusable()
        .cursor(Cursor::Pointer)
        .icon_source(source)
        .icon_size(tokens::ICON_SM)
        .icon_stroke_width(2.0)
        .text_color(tokens::FOREGROUND)
        .fill(tokens::CARD)
        .default_radius(tokens::RADIUS_SM)
        .default_width(Size::Fixed(SIDEBAR_GROUP_ACTION_SIZE))
        .default_height(Size::Fixed(SIDEBAR_GROUP_ACTION_SIZE))
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .ghost()
        .animate(Timing::SPRING_QUICK)
}

/// Vertical list of nav rows ([`sidebar_menu_button`]s or
/// [`sidebar_menu_item`]s) within a group. Shadcn `SidebarMenu`.
///
/// Rows are separated by a `SPACE_1` gap of their own — shadcn's
/// `gap-1` nav list. For flush rows (a file tree, a channel list) that
/// gap has to go: `.gap(0.0)` on the result.
#[track_caller]
pub fn sidebar_menu<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .gap(tokens::SPACE_1)
}

/// Row wrapper for a single menu entry — use for custom row anatomy that
/// [`sidebar_menu_button`] doesn't cover. Shadcn `SidebarMenuItem`.
#[track_caller]
pub fn sidebar_menu_item(child: impl Into<El>) -> El {
    row([child.into()])
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Center)
}

/// Options for [`sidebar_menu_button_with`] — the leading-icon /
/// trailing slots of a nav row (same shape as `TreeItemOpts`).
#[derive(Clone, Debug, Default)]
pub struct SidebarMenuButtonOpts {
    /// Small muted leading glyph before the label. Rendered at
    /// [`tokens::ICON_SM`] in [`tokens::MUTED_FOREGROUND`]. Also what
    /// the row keeps in collapsed mode — rail-visible rows want one.
    pub icon: Option<IconSource>,
    /// Trailing slot at the row's right edge — a count chip
    /// (`badge("7")`), status glyphs, a cluster in a hugging
    /// `row([...])`. The label fills the width between icon and this
    /// slot. Oracle: shadcn `SidebarMenuBadge` / `SidebarMenuAction`
    /// (absolutely positioned there; a flex slot here). Dropped in
    /// collapsed mode, as in the oracle.
    pub trailing: Option<El>,
}

impl SidebarMenuButtonOpts {
    /// Set the leading glyph (see [`SidebarMenuButtonOpts::icon`]).
    pub fn icon(mut self, source: impl IntoIconSource) -> Self {
        self.icon = Some(source.into_icon_source());
        self
    }

    /// Set the trailing slot (see
    /// [`SidebarMenuButtonOpts::trailing`]).
    pub fn trailing(mut self, trailing: impl Into<El>) -> Self {
        self.trailing = Some(trailing.into());
        self
    }
}

/// Focusable nav row — shadcn `SidebarMenuButton`. `current: true`
/// renders the selected (current-page) treatment; otherwise it renders
/// ghost. Add `.key(...)` so clicks route to the app — the app owns
/// which row is current.
#[track_caller]
pub fn sidebar_menu_button(label: impl Into<String>, current: bool) -> El {
    sidebar_menu_button_with(label, current, SidebarMenuButtonOpts::default())
        .at_loc(Location::caller())
}

/// Like [`sidebar_menu_button`], with a small muted leading icon.
#[track_caller]
pub fn sidebar_menu_button_with_icon(
    source: impl IntoIconSource,
    label: impl Into<String>,
    current: bool,
) -> El {
    sidebar_menu_button_with(label, current, SidebarMenuButtonOpts::default().icon(source))
        .at_loc(Location::caller())
}

/// [`sidebar_menu_button`] with the full slot anatomy: optional
/// leading icon, ellipsizing label, optional trailing slot (count
/// chip, status glyphs) per [`SidebarMenuButtonOpts`].
#[track_caller]
pub fn sidebar_menu_button_with(
    label: impl Into<String>,
    current: bool,
    opts: SidebarMenuButtonOpts,
) -> El {
    let mut children: Vec<El> = Vec::with_capacity(3);
    if let Some(source) = opts.icon {
        children.push(
            icon(source)
                .icon_size(tokens::ICON_SM)
                .color(tokens::MUTED_FOREGROUND),
        );
    }
    children.push(sidebar_menu_label(label));
    if let Some(trailing) = opts.trailing {
        children.push(trailing);
    }

    let button = El::new(Kind::Custom("sidebar_menu_button"))
        .at_loc(Location::caller())
        .children(children)
        .axis(Axis::Row)
        .style_profile(StyleProfile::Solid)
        .metrics_role(MetricsRole::ListItem)
        .role(Role::Button)
        .focusable()
        .cursor(Cursor::Pointer)
        .fill(tokens::CARD)
        .default_radius(tokens::RADIUS_SM)
        .default_gap(tokens::SPACE_2)
        .default_padding(Sides::xy(tokens::SPACE_3, 0.0))
        .default_height(Size::Fixed(40.0))
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .width(Size::Fill(1.0))
        .align(Align::Center);
    let styled = if current {
        button.current()
    } else {
        button.ghost()
    };
    styled.animate(Timing::SPRING_QUICK)
}

/// Label text inside a menu row — medium-weight, ellipsizing, fills the
/// remaining row width.
#[track_caller]
pub fn sidebar_menu_label(label: impl Into<String>) -> El {
    text(label)
        .at_loc(Location::caller())
        .label()
        .font_weight(FontWeight::Medium)
        .ellipsis()
        .width(Size::Fill(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::badge::badge;

    #[test]
    fn sidebar_uses_standard_width_and_panel_surface() {
        let s = sidebar([sidebar_header([text("Damascene")])]);

        assert_eq!(s.width, Size::Fixed(tokens::SIDEBAR_WIDTH));
        assert_eq!(s.height, Size::Fill(1.0));
        assert_eq!(s.surface_role, SurfaceRole::Panel);
        assert_eq!(s.fill, Some(tokens::CARD));
    }

    #[test]
    fn sidebar_with_default_opts_is_the_expanded_recipe() {
        let plain = sidebar([sidebar_menu_button("Overview", true)]);
        let with = sidebar_with(
            [sidebar_menu_button("Overview", true)],
            SidebarOpts::default(),
        );

        assert_eq!(with.width, plain.width);
        assert_eq!(with.padding, plain.padding);
        assert_eq!(with.gap, plain.gap);
        assert_eq!(with.surface_role, plain.surface_role);
        assert_eq!(with.children.len(), plain.children.len());
        // The menu button keeps its expanded anatomy: label present.
        assert_eq!(with.children[0].children[0].text.as_deref(), Some("Overview"));
    }

    #[test]
    fn header_footer_and_content_form_the_pinned_slot_anatomy() {
        let header = sidebar_header([text("Northgate Ops")]);
        let footer = sidebar_footer([text("kepner")]);
        let content = sidebar_content([sidebar_group([sidebar_group_label("Channels")])]);

        // Header and footer are hugging strips; identical shells
        // (shadcn's SidebarHeader / SidebarFooter are the same box).
        for slot in [&header, &footer] {
            assert_eq!(slot.width, Size::Fill(1.0));
            assert_eq!(slot.height, Size::Hug);
            assert_eq!(slot.gap, tokens::SPACE_1);
        }
        // Content is the flex-1 middle — the thing that pins the
        // footer to the bottom of the rail.
        assert_eq!(content.width, Size::Fill(1.0));
        assert_eq!(content.height, Size::Fill(1.0));
        assert_eq!(content.gap, tokens::SPACE_4);
    }

    #[test]
    fn sidebar_group_label_reads_as_noninteractive_heading() {
        let label = sidebar_group_label("Foundations");

        assert!(label.key.is_none());
        assert!(!label.focusable);
        assert_eq!(label.cursor, None);
        assert_eq!(label.padding.left, 0.0);
        assert_eq!(label.padding.right, tokens::SPACE_2);
        assert_eq!(label.padding.top, tokens::SPACE_1);
        assert_eq!(label.padding.bottom, 0.0);
    }

    #[test]
    fn group_action_is_a_small_ghost_icon_square() {
        let action = sidebar_group_action("plus").key("add");

        assert_eq!(action.kind, Kind::Custom("sidebar_group_action"));
        assert_eq!(action.width, Size::Fixed(SIDEBAR_GROUP_ACTION_SIZE));
        assert_eq!(action.height, Size::Fixed(SIDEBAR_GROUP_ACTION_SIZE));
        assert!(action.focusable);
        assert_eq!(action.cursor, Some(Cursor::Pointer));
        assert!(action.fill.is_none(), "ghost until hovered");
        assert!(action.animate_timing().is_some());

        // Hugs the right edge when composed after the filling label.
        let heading = row([sidebar_group_label("Channels"), sidebar_group_action("plus")])
            .align(Align::Center);
        assert_eq!(heading.children[0].width, Size::Fill(1.0));
        assert_eq!(
            heading.children[1].width,
            Size::Fixed(SIDEBAR_GROUP_ACTION_SIZE)
        );
    }

    #[test]
    fn sidebar_menu_button_uses_list_density_and_current_treatment() {
        let current = sidebar_menu_button_with_icon("layout-dashboard", "Overview", true);
        let inactive = sidebar_menu_button("Settings", false);

        assert_eq!(current.metrics_role, Some(MetricsRole::ListItem));
        assert_eq!(current.align, Align::Center);
        assert_eq!(current.height, Size::Fixed(40.0));
        assert_eq!(current.surface_role, SurfaceRole::Current);
        assert!(current.focusable);
        assert_eq!(current.paint_overflow, Sides::all(tokens::RING_WIDTH));
        assert_eq!(current.hit_overflow, Sides::all(tokens::HIT_OVERFLOW));
        assert!(
            current.animate_timing().is_some(),
            "current changes should ease"
        );
        assert_eq!(inactive.height, Size::Fixed(40.0));
        assert_eq!(inactive.padding, Sides::xy(tokens::SPACE_3, 0.0));
        assert_eq!(inactive.paint_overflow, Sides::all(tokens::RING_WIDTH));
        assert_eq!(inactive.hit_overflow, Sides::all(tokens::HIT_OVERFLOW));
        assert!(inactive.fill.is_none());
        assert!(
            inactive.animate_timing().is_some(),
            "inactive changes should ease"
        );
    }

    #[test]
    fn trailing_slot_hugs_after_the_filling_label() {
        let row = sidebar_menu_button_with(
            "War Room",
            false,
            SidebarMenuButtonOpts::default()
                .icon("volume-2")
                .trailing(badge("7")),
        );

        assert_eq!(row.children.len(), 3);
        assert_eq!(row.children[0].kind, Kind::Custom("icon"));
        let label = &row.children[1];
        assert_eq!(label.text.as_deref(), Some("War Room"));
        assert_eq!(
            label.width,
            Size::Fill(1.0),
            "the label absorbs slack; the trailing slot keeps its own width"
        );
        let trailing = row.children.last().unwrap();
        assert_eq!(trailing.kind, Kind::Badge);
        assert_eq!(trailing.width, Size::Hug, "trailing slot is not resized");

        // The dedicated variants stay thin wrappers over the opts form.
        let bare = sidebar_menu_button("Settings", false);
        assert_eq!(bare.children.len(), 1, "label only");
        let iconed = sidebar_menu_button_with_icon("settings", "Settings", false);
        assert_eq!(iconed.children.len(), 2, "icon + label");
        assert_eq!(iconed.children[0].kind, Kind::Custom("icon"));
    }

    #[test]
    fn collapsed_rail_is_icon_width_with_square_icon_buttons() {
        let rail = sidebar_with(
            [
                sidebar_header([row([
                    icon("audio-lines"),
                    text("Northgate Ops").title(),
                ])]),
                sidebar_content([sidebar_group([
                    row([
                        sidebar_group_label("Channels"),
                        sidebar_group_action("plus").key("add"),
                    ]),
                    sidebar_menu([sidebar_menu_button_with(
                        "War Room",
                        true,
                        SidebarMenuButtonOpts::default()
                            .icon("volume-2")
                            .trailing(badge("7")),
                    )
                    .key("ch:war")]),
                ])]),
                sidebar_footer([row([text("kepner"), icon("mic")])]),
            ],
            SidebarOpts::default().collapsed(true),
        );

        // Rail geometry: icon width, tighter gutter, same panel surface.
        assert_eq!(rail.width, Size::Fixed(tokens::SIDEBAR_WIDTH_ICON));
        assert_eq!(rail.height, Size::Fill(1.0));
        assert_eq!(rail.surface_role, SurfaceRole::Panel);
        assert_eq!(rail.padding, Sides::all(tokens::SPACE_2));

        // Header: title text dropped, icon kept; the horizontal
        // cluster restacks vertically for the 32 px content column.
        let header_row = &rail.children[0].children[0];
        assert_eq!(header_row.children.len(), 1);
        assert_eq!(header_row.children[0].kind, Kind::Custom("icon"));
        assert_eq!(header_row.axis, Axis::Column, "rows restack in the rail");
        assert_eq!(header_row.align, Align::Center);

        // Group heading row: label and action both dropped.
        let group = &rail.children[1].children[0];
        let heading = &group.children[0];
        assert!(heading.children.is_empty(), "label + action hidden");

        // Menu button: square, icon only, key + current kept, trailing
        // badge dropped.
        let button = &group.children[1].children[0];
        assert_eq!(button.kind, Kind::Custom("sidebar_menu_button"));
        assert_eq!(button.width, Size::Fixed(tokens::CONTROL_HEIGHT));
        assert_eq!(button.height, Size::Fixed(tokens::CONTROL_HEIGHT));
        assert_eq!(button.padding, Sides::zero());
        assert_eq!(button.children.len(), 1);
        assert_eq!(button.children[0].kind, Kind::Custom("icon"));
        assert_eq!(button.key.as_deref(), Some("ch:war"));
        assert!(button.focusable);
        assert_eq!(button.surface_role, SurfaceRole::Current);

        // Footer: name dropped, mic icon kept.
        let footer_row = &rail.children[2].children[0];
        assert_eq!(footer_row.children.len(), 1);
        assert_eq!(footer_row.children[0].kind, Kind::Custom("icon"));
    }

    #[test]
    fn collapsed_button_without_icon_keeps_its_blank_square() {
        // Oracle behavior: shadcn clips the label out of the same
        // square rather than hiding the row — the click target and
        // current marker survive.
        let rail = sidebar_with(
            [sidebar_menu([sidebar_menu_button("Settings", false).key("s")])],
            SidebarOpts::default().collapsed(true),
        );

        let button = &rail.children[0].children[0];
        assert_eq!(button.kind, Kind::Custom("sidebar_menu_button"));
        assert!(button.children.is_empty(), "label dropped, no icon to keep");
        assert_eq!(button.width, Size::Fixed(tokens::CONTROL_HEIGHT));
        assert_eq!(button.key.as_deref(), Some("s"));
        assert!(button.focusable, "still a live click target");
    }
}
