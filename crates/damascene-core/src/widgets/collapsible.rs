//! Collapsible — one standalone disclosure section (a trigger row that
//! reveals a body).
//!
//! # Oracle (`docs/NAMING_ORACLE.md`)
//!
//! shadcn/ui's **`Collapsible`** (registry item `collapsible`, a
//! re-export of Radix's `Collapsible.Root` / `CollapsibleTrigger` /
//! `CollapsibleContent`) — [`collapsible`], [`collapsible_trigger`],
//! [`collapsible_content`]. Radix's `open` prop keeps its name for the
//! app-owned bool. [`collapsible_trigger_with_icon`] and
//! [`collapsible_with_icon`] are ours, minted off the neighbouring
//! [`accordion_trigger_with_icon`](crate::widgets::accordion::accordion_trigger_with_icon)
//! rather than an oracle name (shadcn puts a leading glyph in the
//! trigger's JSX children, which our `label: impl Into<String>`
//! signature has no slot for).
//!
//! # Shape
//!
//! Collapsible is accordion's **single-item sibling**, and the layering
//! is the oracle's: Radix builds `Accordion` out of `Collapsible`, so
//! the disclosure recipe lives here and
//! [`accordion_trigger`](crate::widgets::accordion::accordion_trigger) /
//! [`accordion_content`](crate::widgets::accordion::accordion_content) delegate to it. The
//! two differ only in the routed key — an accordion trigger is keyed
//! `{key}:accordion:{value}` because one accordion key covers many
//! items, while a collapsible has exactly one trigger and so is keyed
//! by the app's `key` verbatim (like [`tree_item`](crate::widgets::tree::tree_item)).
//!
//! **Which disclosure to reach for:**
//!
//! - [`collapsible`] — a standalone disclosure section: a settings
//!   group, a chat message's collapsed reasoning, a sidebar section
//!   header. Nothing else closes when it opens.
//! - [`accordion`](crate::widgets::accordion) — a *stack* of sections where the
//!   app closes the others as one opens (`Option<V>` state, folded by
//!   [`accordion::apply_event`](crate::widgets::accordion::apply_event)).
//! - [`tree`](crate::widgets::tree) — hierarchical rows on the 22px chrome rung,
//!   with a disclosure gutter, depth indent and arrow-key navigation.
//!   A channel roster or file explorer is a tree; the collapsible
//!   *section* that might contain one is a collapsible.
//!
//! # State
//!
//! Per the library's controlled-widget contract the app owns `open`.
//! Clicking the trigger (or Enter/Space on it) emits the normal keyed
//! `Click` / `Activate`; the app flips its own bool and rebuilds —
//! typically through [`apply_event`], the same `&mut bool` shape as
//! [`checkbox::apply_event`](crate::widgets::checkbox::apply_event) and
//! [`switch::apply_event`](crate::widgets::switch::apply_event).
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct Panel { advanced_open: bool }
//!
//! impl App for Panel {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         collapsible("advanced", "Advanced", self.advanced_open, [
//!             field("Log level", select_trigger("log", "Warn")),
//!             field("Telemetry", switch("telemetry", false)),
//!         ])
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
//!         collapsible::apply_event(&mut self.advanced_open, &event, "advanced");
//!     }
//! }
//! ```

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::a11y::Role;
use crate::anim::Timing;
use crate::cursor::Cursor;
use crate::event::UiEvent;
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::text;
use crate::{IntoIconSource, icon};

/// Fold a routed [`UiEvent`] into the app-owned `open` bool
/// (controlled-widget contract): a `Click` / `Activate` on the
/// trigger's `key` flips it. Returns `true` when the event belonged to
/// this collapsible.
///
/// The single-item sibling of
/// [`accordion::apply_event`](crate::widgets::accordion::apply_event) — an
/// accordion folds into `Option<V>` (opening one item closes the
/// rest), a collapsible into a plain `bool`, exactly like
/// [`checkbox::apply_event`](crate::widgets::checkbox::apply_event).
pub fn apply_event(open: &mut bool, event: &UiEvent, key: &str) -> bool {
    if event.is_click_or_activate(key) {
        *open = !*open;
        return true;
    }
    false
}

/// The disclosure recipe shared by every trigger in the family
/// (collapsible's two, and accordion's two by delegation): an
/// optional leading glyph, an ellipsizing label that absorbs the
/// slack, and a chevron reflecting `open`.
fn trigger_row(key: &str, leading: Option<El>, label: impl Into<String>, open: bool) -> El {
    let mut children: Vec<El> = Vec::with_capacity(3);
    if let Some(leading) = leading {
        children.push(leading);
    }
    children.push(
        text(label)
            .label()
            .font_weight(FontWeight::Medium)
            .ellipsis()
            .width(Size::Fill(1.0)),
    );
    children.push(
        icon(if open {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        })
        .icon_size(tokens::ICON_XS)
        .color(tokens::MUTED_FOREGROUND),
    );

    row(children)
        .key(key)
        .style_profile(StyleProfile::Solid)
        .metrics_role(MetricsRole::ListItem)
        // ARIA disclosure pattern: the trigger is a button announcing
        // its expanded state.
        .role(Role::Button)
        .aria_expanded(open)
        .focusable()
        .cursor(Cursor::Pointer)
        .fill(tokens::CARD)
        .default_radius(tokens::RADIUS_SM)
        .default_gap(tokens::SPACE_2)
        .default_padding(Sides::xy(tokens::SPACE_3, 0.0))
        .default_height(Size::Fixed(40.0))
        // Disclosure triggers stack flush (a gapless accordion column,
        // or sections butted against their own content), so an outside
        // ring band and hit-target outset would land on the neighbour.
        // Inside ring + no hit overflow is the stock recipe for
        // tightly-stacked focusable rows (issue #119).
        .focus_ring_inside()
        .axis(Axis::Row)
        .align(Align::Center)
        .width(Size::Fill(1.0))
        .animate(Timing::SPRING_QUICK)
}

/// One disclosure section (shadcn's `Collapsible`): a
/// [`collapsible_trigger`] plus, when `open`, a [`collapsible_content`]
/// with the children. The app passes `open` from its own state and
/// folds trigger events back through [`apply_event`].
#[track_caller]
pub fn collapsible<I, E>(key: &str, label: impl Into<String>, open: bool, children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    section(collapsible_trigger(key, label, open), open, children).at_loc(Location::caller())
}

/// [`collapsible`] with a leading icon before the trigger's label.
#[track_caller]
pub fn collapsible_with_icon<I, E>(
    key: &str,
    source: impl IntoIconSource,
    label: impl Into<String>,
    open: bool,
    children: I,
) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    section(
        collapsible_trigger_with_icon(key, source, label, open),
        open,
        children,
    )
    .at_loc(Location::caller())
}

/// The trigger-plus-optional-content column both `collapsible*`
/// constructors return.
fn section<I, E>(trigger: El, open: bool, children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut body = vec![trigger];
    if open {
        body.push(collapsible_content(children));
    }
    column(body)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .gap(0.0)
}

/// The clickable header row (shadcn's `CollapsibleTrigger`) — label
/// plus a chevron reflecting `open`. Keyed by `key` verbatim;
/// activation routes through [`apply_event`].
#[track_caller]
pub fn collapsible_trigger(key: &str, label: impl Into<String>, open: bool) -> El {
    trigger_row(key, None, label, open).at_loc(Location::caller())
}

/// [`collapsible_trigger`] with a leading icon before the label — the
/// shape a sidebar section header or a channel group wants.
#[track_caller]
pub fn collapsible_trigger_with_icon(
    key: &str,
    source: impl IntoIconSource,
    label: impl Into<String>,
    open: bool,
) -> El {
    let leading = icon(source)
        .icon_size(tokens::ICON_SM)
        .color(tokens::MUTED_FOREGROUND);
    trigger_row(key, Some(leading), label, open).at_loc(Location::caller())
}

/// Body revealed under an open trigger (shadcn's `CollapsibleContent`)
/// — an indented column; [`collapsible`] renders it only while `open`.
#[track_caller]
pub fn collapsible_content<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .role(Role::Group)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .padding(Sides {
            left: tokens::SPACE_2,
            right: tokens::SPACE_2,
            top: 0.0,
            bottom: tokens::SPACE_3,
        })
        .gap(tokens::SPACE_2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyModifiers, UiEvent, UiEventKind};
    use crate::widgets::accordion::{accordion_trigger, accordion_trigger_with_icon};

    fn click_event(key: &str) -> UiEvent {
        UiEvent {
            path: None,
            kind: UiEventKind::Click,
            key: Some(key.to_string()),
            target: None,
            pointer: None,
            key_press: None,
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 1,
            pointer_kind: None,
            wheel_delta: None,
        }
    }

    #[test]
    fn trigger_routes_by_the_plain_key_and_centers_row() {
        // Unlike an accordion item, a collapsible has exactly one
        // trigger — so its key is the app's key verbatim, with no
        // `:accordion:{value}` discriminator to compose.
        let trigger = collapsible_trigger("advanced", "Advanced", false);

        assert_eq!(trigger.key.as_deref(), Some("advanced"));
        assert_eq!(trigger.metrics_role, Some(MetricsRole::ListItem));
        assert_eq!(trigger.align, Align::Center);
        assert_eq!(trigger.width, Size::Fill(1.0));
        assert!(trigger.focusable);
        assert_eq!(trigger.cursor, Some(Cursor::Pointer));
        // Flush stacking demands an inside ring and no hit-target
        // outset — see `trigger_row` (issue #119).
        assert_eq!(trigger.focus_ring_placement, FocusRingPlacement::Inside);
        assert_eq!(trigger.hit_overflow, Sides::default());
        assert!(trigger.animate_timing().is_some());
    }

    #[test]
    fn chevron_reflects_open() {
        let closed = collapsible_trigger("advanced", "Advanced", false);
        let open = collapsible_trigger("advanced", "Advanced", true);

        assert_eq!(
            closed.children.last().unwrap().icon,
            Some(crate::IconSource::Builtin(IconName::ChevronRight))
        );
        assert_eq!(
            open.children.last().unwrap().icon,
            Some(crate::IconSource::Builtin(IconName::ChevronDown))
        );
        // The chevron is display-only: the whole row carries the key.
        assert_eq!(open.children.last().unwrap().key, None);
    }

    #[test]
    fn body_is_present_only_when_open() {
        let closed = collapsible("advanced", "Advanced", false, [text("Body")]);
        let open = collapsible("advanced", "Advanced", true, [text("Body")]);

        assert_eq!(closed.children.len(), 1, "closed renders the trigger only");
        assert_eq!(open.children.len(), 2);
        assert_eq!(open.children[1].children[0].text.as_deref(), Some("Body"));
        assert_eq!(open.children[1].padding.left, tokens::SPACE_2);
        assert_eq!(open.children[1].padding.bottom, tokens::SPACE_3);
        assert_eq!(open.gap, 0.0, "trigger and body meet flush");
    }

    #[test]
    fn leading_icon_slots_before_the_filling_label() {
        let bare = collapsible_trigger("k", "Advanced", true);
        let with_icon = collapsible_trigger_with_icon("k", "folder", "Advanced", true);

        assert_eq!(bare.children.len(), 2, "no icon slot unless asked for");
        assert_eq!(with_icon.children.len(), 3);
        assert_eq!(with_icon.children[0].kind, Kind::Custom("icon"));
        assert_eq!(
            with_icon.children[0].text_color,
            Some(tokens::MUTED_FOREGROUND)
        );
        let label = &with_icon.children[1];
        assert_eq!(label.text.as_deref(), Some("Advanced"));
        assert_eq!(label.text_overflow, TextOverflow::Ellipsis);
        assert_eq!(
            label.width,
            Size::Fill(1.0),
            "the label absorbs slack; the chevron keeps its own width"
        );

        // …and the `_with_icon` container threads the same slot.
        let section = collapsible_with_icon("k", "folder", "Advanced", true, [text("Body")]);
        assert_eq!(section.children[0].children.len(), 3);
        assert_eq!(section.children.len(), 2);
    }

    #[test]
    fn apply_event_flips_the_app_owned_bool() {
        let mut open = false;

        assert!(apply_event(&mut open, &click_event("advanced"), "advanced"));
        assert!(open);
        assert!(apply_event(&mut open, &click_event("advanced"), "advanced"));
        assert!(!open);

        // Foreign keys and non-activation kinds are left alone.
        assert!(!apply_event(&mut open, &click_event("other"), "advanced"));
        assert!(!open);
        let mut down = click_event("advanced");
        down.kind = UiEventKind::PointerDown;
        assert!(!apply_event(&mut open, &down, "advanced"));
        assert!(!open);
    }

    #[test]
    fn accordion_triggers_are_this_recipe_under_a_composed_key() {
        // Accordion delegates to the collapsible recipe (Radix builds
        // `Accordion` out of `Collapsible`); the only difference is the
        // routed key. If the two ever drift, this fails.
        let mine = collapsible_trigger("settings:accordion:security", "Security", false);
        let theirs = accordion_trigger("settings", "security", "Security", false);

        assert_eq!(mine.key, theirs.key);
        assert_eq!(mine.height, theirs.height);
        assert_eq!(mine.padding, theirs.padding);
        assert_eq!(mine.gap, theirs.gap);
        assert_eq!(mine.fill, theirs.fill);
        assert_eq!(mine.radius, theirs.radius);
        assert_eq!(mine.style_profile, theirs.style_profile);
        assert_eq!(mine.focus_ring_placement, theirs.focus_ring_placement);
        assert_eq!(mine.children.len(), theirs.children.len());
        assert_eq!(mine.children[1].icon, theirs.children[1].icon);

        let mine_icon =
            collapsible_trigger_with_icon("settings:accordion:security", "lock", "Security", true);
        let theirs_icon =
            accordion_trigger_with_icon("settings", "security", "lock", "Security", true);
        assert_eq!(mine_icon.key, theirs_icon.key);
        assert_eq!(mine_icon.children.len(), theirs_icon.children.len());
        assert_eq!(mine_icon.children[0].icon, theirs_icon.children[0].icon);
        assert_eq!(
            mine_icon.children[0].font_size, theirs_icon.children[0].font_size,
            "leading glyphs render at the same size"
        );
    }

    #[test]
    fn stacked_collapsibles_keep_their_rings_and_hit_targets_clear_issue_119() {
        // The standalone-disclosure equivalent of the accordion lint
        // regression: sections butted together in a column, one open,
        // must not collide ring bands or hit outsets.
        let mut root = column([
            collapsible("a", "Section A", true, [text("body")]),
            collapsible("b", "Section B", false, [text("body")]),
            collapsible_with_icon("c", "folder", "Section C", false, [text("body")]),
        ])
        .gap(0.0)
        .width(Size::Fill(1.0));

        let mut ui_state = crate::UiState::new();
        crate::layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 400.0, 400.0));
        let report = crate::bundle::lint::lint(&root, &ui_state, &crate::theme::Theme::default());
        assert!(
            !report.findings.iter().any(|f| matches!(
                f.kind,
                crate::bundle::lint::FindingKind::HitOverflowCollision
                    | crate::bundle::lint::FindingKind::FocusRingObscured
            )),
            "{}",
            report.text()
        );
    }
}
