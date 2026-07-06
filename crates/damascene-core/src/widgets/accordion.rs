//! Accordion / collapsible anatomy.
//!
//! This is a controlled widget: the app owns which item is open and
//! folds routed trigger events through [`apply_event`], matching the
//! same pattern as tabs, select, switch, and radio.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::anim::Timing;
use crate::cursor::Cursor;
use crate::event::{UiEvent, UiEventKind};
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::separator::separator;
use crate::widgets::text::text;
use crate::{IntoIconSource, icon};

/// What a routed [`UiEvent`] means for a controlled accordion keyed
/// `key`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AccordionAction<'a> {
    /// A `{key}:accordion:{value}` trigger was activated; carries the
    /// item's `value`.
    Toggle(&'a str),
}

/// The routed key for an accordion trigger: `{key}:accordion:{value}`.
/// Matches what [`accordion_trigger`] stamps on the row — useful for
/// tests and custom event routing.
pub fn accordion_item_key(key: &str, value: &impl std::fmt::Display) -> String {
    format!("{key}:accordion:{value}")
}

/// Classify a routed [`UiEvent`] against an accordion keyed `key`.
/// Returns `None` for events that don't belong to this accordion (or
/// aren't `Click`/`Activate`).
pub fn classify_event<'a>(event: &'a UiEvent, key: &str) -> Option<AccordionAction<'a>> {
    if !matches!(event.kind, UiEventKind::Click | UiEventKind::Activate) {
        return None;
    }
    let routed = event.route()?;
    let rest = routed.strip_prefix(key)?.strip_prefix(':')?;
    let value = rest.strip_prefix("accordion:")?;
    Some(AccordionAction::Toggle(value))
}

/// Fold a routed trigger event into the app-owned open value
/// (controlled-widget contract): toggling the already-open item closes
/// it, any other item opens it. `parse` converts the routed string
/// back into the app's value type. Returns `true` when the event
/// belonged to this accordion (even if `parse` rejected the value).
pub fn apply_event<V>(
    open: &mut Option<V>,
    event: &UiEvent,
    key: &str,
    parse: impl FnOnce(&str) -> Option<V>,
) -> bool
where
    V: std::fmt::Display + PartialEq,
{
    let Some(AccordionAction::Toggle(raw)) = classify_event(event, key) else {
        return false;
    };
    let Some(next) = parse(raw) else {
        return true;
    };
    if open.as_ref() == Some(&next) {
        *open = None;
    } else {
        *open = Some(next);
    }
    true
}

/// The accordion container (shadcn's `Accordion`) — a full-width,
/// gapless column of [`accordion_item`]s.
///
/// Items stack flush by design, so the triggers keep their focus
/// ring *inside* their bounds (like dropdown-menu items) — no edge
/// gutter or inter-item gap is needed for ring or hit-target room
/// (issue #119).
#[track_caller]
pub fn accordion<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .gap(0.0)
}

/// One accordion entry (shadcn's `AccordionItem`): an
/// [`accordion_trigger`] plus, when `open`, an [`accordion_content`]
/// with the children. The app passes `open` from its owned state.
#[track_caller]
pub fn accordion_item<I, E>(
    key: &str,
    value: impl std::fmt::Display,
    label: impl Into<String>,
    open: bool,
    children: I,
) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut body = vec![accordion_trigger(key, value, label, open)];
    if open {
        body.push(accordion_content(children));
    }
    column(body)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .gap(0.0)
}

/// The clickable header row (shadcn's `AccordionTrigger`) — label plus
/// a chevron reflecting `open`. Keyed `{key}:accordion:{value}`;
/// activation routes through [`apply_event`] / [`classify_event`].
#[track_caller]
pub fn accordion_trigger(
    key: &str,
    value: impl std::fmt::Display,
    label: impl Into<String>,
    open: bool,
) -> El {
    let chevron = if open {
        "chevron-down"
    } else {
        "chevron-right"
    };
    row([
        text(label)
            .label()
            .font_weight(FontWeight::Medium)
            .ellipsis()
            .width(Size::Fill(1.0)),
        icon(chevron)
            .icon_size(tokens::ICON_XS)
            .color(tokens::MUTED_FOREGROUND),
    ])
    .at_loc(Location::caller())
    .key(accordion_item_key(key, &value))
    .style_profile(StyleProfile::Solid)
    .metrics_role(MetricsRole::ListItem)
    .focusable()
    .cursor(Cursor::Pointer)
    .fill(tokens::CARD)
    .default_radius(tokens::RADIUS_SM)
    .default_gap(tokens::SPACE_2)
    .default_padding(Sides::xy(tokens::SPACE_3, 0.0))
    .default_height(Size::Fixed(40.0))
    // Triggers stack flush (gapless column, or separated by a 1px
    // rule), so an outside ring band and hit-target outset would
    // land on the neighboring trigger. Inside ring + no hit
    // overflow is the stock recipe for tightly-stacked focusable
    // rows (dropdown/menubar items) — issue #119.
    .focus_ring_inside()
    .axis(Axis::Row)
    .align(Align::Center)
    .width(Size::Fill(1.0))
    .animate(Timing::SPRING_QUICK)
}

/// [`accordion_trigger`] with a leading icon before the label.
#[track_caller]
pub fn accordion_trigger_with_icon(
    key: &str,
    value: impl std::fmt::Display,
    source: impl IntoIconSource,
    label: impl Into<String>,
    open: bool,
) -> El {
    let chevron = if open {
        "chevron-down"
    } else {
        "chevron-right"
    };
    row([
        icon(source)
            .icon_size(tokens::ICON_SM)
            .color(tokens::MUTED_FOREGROUND),
        text(label)
            .label()
            .font_weight(FontWeight::Medium)
            .ellipsis()
            .width(Size::Fill(1.0)),
        icon(chevron)
            .icon_size(tokens::ICON_XS)
            .color(tokens::MUTED_FOREGROUND),
    ])
    .at_loc(Location::caller())
    .key(accordion_item_key(key, &value))
    .style_profile(StyleProfile::Solid)
    .metrics_role(MetricsRole::ListItem)
    .focusable()
    .cursor(Cursor::Pointer)
    .fill(tokens::CARD)
    .default_radius(tokens::RADIUS_SM)
    .default_gap(tokens::SPACE_2)
    .default_padding(Sides::xy(tokens::SPACE_3, 0.0))
    .default_height(Size::Fixed(40.0))
    // Same flush-stacking recipe as `accordion_trigger` (issue #119).
    .focus_ring_inside()
    .axis(Axis::Row)
    .align(Align::Center)
    .width(Size::Fill(1.0))
    .animate(Timing::SPRING_QUICK)
}

/// Body shown under an open trigger (shadcn's `AccordionContent`) —
/// an indented column; [`accordion_item`] renders it only while open.
#[track_caller]
pub fn accordion_content<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
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

/// Hairline rule between accordion items — the stock
/// [`crate::separator`] under the anatomy's name.
#[track_caller]
pub fn accordion_separator() -> El {
    separator()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyModifiers, UiEvent};

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
    fn accordion_trigger_routes_by_stable_key_and_centers_row() {
        let trigger = accordion_trigger("settings", "security", "Security", false);

        assert_eq!(trigger.key.as_deref(), Some("settings:accordion:security"));
        assert_eq!(trigger.metrics_role, Some(MetricsRole::ListItem));
        assert_eq!(trigger.align, Align::Center);
        assert!(trigger.focusable);
        // Flush stacking demands an inside ring and no hit-target
        // outset — an outside band would collide with the adjacent
        // trigger (issue #119).
        assert_eq!(trigger.focus_ring_placement, FocusRingPlacement::Inside);
        assert_eq!(trigger.hit_overflow, Sides::default());
        assert_eq!(
            trigger.children[1].icon,
            Some(crate::IconSource::Builtin(IconName::ChevronRight))
        );
    }

    #[test]
    fn accordion_item_includes_content_only_when_open() {
        let closed = accordion_item("settings", "security", "Security", false, [text("Body")]);
        let open = accordion_item("settings", "security", "Security", true, [text("Body")]);

        assert_eq!(closed.children.len(), 1);
        assert_eq!(open.children.len(), 2);
        assert_eq!(open.children[1].padding.bottom, tokens::SPACE_3);
    }

    #[test]
    fn flush_accordion_compositions_pass_their_own_lint_issue_119() {
        // The three stock shapes that used to trip the bundle lint
        // (issue #119): adjacent collapsed triggers (outside ring
        // bands and hit outsets landed on the flush neighbor),
        // separator-interleaved triggers (a 1px rule can't absorb a
        // 2px band), and an open item whose content paints flush
        // under the trigger's bottom edge.
        let all_collapsed = accordion([
            accordion_item("k", "a", "Section A", false, [text("body")]),
            accordion_item("k", "b", "Section B", false, [text("body")]),
            accordion_item("k", "c", "Section C", false, [text("body")]),
        ]);
        let separated = accordion([
            accordion_item("k", "a", "Section A", false, [text("body")]),
            accordion_separator(),
            accordion_item("k", "b", "Section B", false, [text("body")]),
        ]);
        let open_adjacent = accordion([
            accordion_item("k", "a", "Section A", true, [text("body")]),
            accordion_item("k", "b", "Section B", false, [text("body")]),
        ]);

        for (shape, mut root) in [
            ("all collapsed", all_collapsed),
            ("separator-interleaved", separated),
            ("open above collapsed", open_adjacent),
        ] {
            let mut ui_state = crate::UiState::new();
            crate::layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 400.0, 300.0));
            let report = crate::bundle::lint::lint(&root, &ui_state);
            assert!(
                !report.findings.iter().any(|f| matches!(
                    f.kind,
                    crate::bundle::lint::FindingKind::HitOverflowCollision
                        | crate::bundle::lint::FindingKind::FocusRingObscured
                )),
                "{shape}: {}",
                report.text()
            );
        }
    }

    #[test]
    fn apply_event_toggles_single_open_value() {
        let mut open = Some("security".to_string());

        assert!(apply_event(
            &mut open,
            &click_event("settings:accordion:security"),
            "settings",
            |raw| Some(raw.to_string()),
        ));
        assert_eq!(open, None);

        assert!(apply_event(
            &mut open,
            &click_event("settings:accordion:billing"),
            "settings",
            |raw| Some(raw.to_string()),
        ));
        assert_eq!(open, Some("billing".to_string()));
    }
}
