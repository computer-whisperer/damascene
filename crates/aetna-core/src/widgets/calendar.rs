//! Calendar — a shadcn-shaped month grid.
//!
//! Apps own the selected day and current month. The widget renders the
//! familiar calendar anatomy — header navigation, weekday row, and
//! focusable day cells — while routing clicks through stable keys.
//!
//! ```ignore
//! use aetna_core::prelude::*;
//!
//! struct DatePicker {
//!     selected: Option<String>,
//! }
//!
//! impl App for DatePicker {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         calendar_month("billing-date", "May 2026", [
//!             CalendarDay::new("2026-05-01", "1"),
//!             CalendarDay::new("2026-05-02", "2").selected(),
//!         ])
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent) {
//!         calendar::apply_event(&mut self.selected, &event, "billing-date");
//!     }
//! }
//! ```
//!
//! # Routed keys
//!
//! - `{key}:prev` — previous-month nav button.
//! - `{key}:next` — next-month nav button.
//! - `{key}:day:{value}` — day cell click / activate.

use std::panic::Location;

use crate::anim::Timing;
use crate::cursor::Cursor;
use crate::event::{UiEvent, UiEventKind};
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::{icon_button, text};

const DAY_SIZE: f32 = 36.0;
const WEEKDAYS: [&str; 7] = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

/// A day cell inside [`calendar_month`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarDay {
    pub value: String,
    pub label: String,
    pub selected: bool,
    pub outside: bool,
    pub disabled: bool,
}

impl CalendarDay {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            selected: false,
            outside: false,
            disabled: false,
        }
    }

    pub fn selected(mut self) -> Self {
        self.selected = true;
        self
    }

    pub fn outside(mut self) -> Self {
        self.outside = true;
        self
    }

    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }
}

/// What a routed [`UiEvent`] means for a controlled calendar keyed
/// `key`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CalendarAction<'a> {
    PreviousMonth,
    NextMonth,
    Pick(&'a str),
}

pub fn classify_event<'a>(event: &'a UiEvent, key: &str) -> Option<CalendarAction<'a>> {
    if !matches!(event.kind, UiEventKind::Click | UiEventKind::Activate) {
        return None;
    }
    let routed = event.route()?;
    let rest = routed.strip_prefix(key)?.strip_prefix(':')?;
    match rest {
        "prev" => Some(CalendarAction::PreviousMonth),
        "next" => Some(CalendarAction::NextMonth),
        _ => rest.strip_prefix("day:").map(CalendarAction::Pick),
    }
}

/// Fold a day-pick event into an app-owned selected day. Previous/next
/// events are consumed but leave the selected value untouched; apps
/// that own month paging should use [`classify_event`] directly.
pub fn apply_event(selected: &mut Option<String>, event: &UiEvent, key: &str) -> bool {
    let Some(action) = classify_event(event, key) else {
        return false;
    };
    if let CalendarAction::Pick(value) = action {
        *selected = Some(value.to_string());
    }
    true
}

pub fn calendar_day_key(key: &str, value: &impl std::fmt::Display) -> String {
    format!("{key}:day:{value}")
}

/// Render a month grid. `days` should normally contain 35 or 42 cells,
/// including outside-month leading/trailing days when needed.
#[track_caller]
pub fn calendar_month<I>(key: impl Into<String>, month_label: impl Into<String>, days: I) -> El
where
    I: IntoIterator<Item = CalendarDay>,
{
    let caller = Location::caller();
    let key = key.into();
    let days: Vec<CalendarDay> = days.into_iter().collect();
    let week_rows = days
        .chunks(7)
        .map(|week| calendar_week_row(caller, &key, week))
        .collect::<Vec<_>>();

    El::new(Kind::Custom("calendar"))
        .at_loc(caller)
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::Panel)
        .axis(Axis::Column)
        .align(Align::Stretch)
        .children([
            calendar_header(caller, &key, month_label.into()),
            calendar_weekdays(caller),
            column(week_rows)
                .at_loc(caller)
                .gap(tokens::SPACE_1)
                .width(Size::Hug)
                .height(Size::Hug),
        ])
        .gap(tokens::SPACE_2)
        .padding(tokens::SPACE_3)
        .fill(tokens::CARD)
        .stroke(tokens::BORDER)
        .default_radius(tokens::RADIUS_MD)
        .width(Size::Hug)
        .height(Size::Hug)
}

fn calendar_header(caller: &'static Location<'static>, key: &str, month_label: String) -> El {
    row([
        icon_button(IconName::ChevronLeft)
            .at_loc(caller)
            .ghost()
            .key(format!("{key}:prev"))
            .width(Size::Fixed(DAY_SIZE))
            .height(Size::Fixed(DAY_SIZE)),
        text(month_label)
            .at_loc(caller)
            .label()
            .semibold()
            .text_align(TextAlign::Center)
            .width(Size::Fill(1.0)),
        icon_button(IconName::ChevronRight)
            .at_loc(caller)
            .ghost()
            .key(format!("{key}:next"))
            .width(Size::Fixed(DAY_SIZE))
            .height(Size::Fixed(DAY_SIZE)),
    ])
    .at_loc(caller)
    .align(Align::Center)
    .gap(tokens::SPACE_1)
    .width(Size::Fill(1.0))
}

fn calendar_weekdays(caller: &'static Location<'static>) -> El {
    row(WEEKDAYS.map(|d| {
        text(d)
            .at_loc(caller)
            .caption()
            .muted()
            .text_align(TextAlign::Center)
            .width(Size::Fixed(DAY_SIZE))
            .height(Size::Fixed(24.0))
    }))
    .at_loc(caller)
    .gap(tokens::SPACE_1)
    .width(Size::Hug)
}

fn calendar_week_row(caller: &'static Location<'static>, key: &str, week: &[CalendarDay]) -> El {
    row(week.iter().map(|day| calendar_day(caller, key, day)))
        .at_loc(caller)
        .gap(tokens::SPACE_1)
        .width(Size::Hug)
        .height(Size::Hug)
}

fn calendar_day(caller: &'static Location<'static>, key: &str, day: &CalendarDay) -> El {
    let base = El::new(Kind::Custom("calendar_day"))
        .at_loc(caller)
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::Button)
        .focusable()
        .focus_ring_inside()
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .cursor(Cursor::Pointer)
        .key(calendar_day_key(key, &day.value))
        .text(day.label.clone())
        .text_align(TextAlign::Center)
        .text_role(TextRole::Label)
        .default_radius(tokens::RADIUS_MD)
        .width(Size::Fixed(DAY_SIZE))
        .height(Size::Fixed(DAY_SIZE))
        .padding(Sides::zero());

    let styled = if day.selected {
        base.current()
    } else {
        base.ghost()
    };
    let styled = if day.outside {
        styled.color(tokens::MUTED_FOREGROUND)
    } else {
        styled
    };
    let styled = if day.disabled {
        styled.disabled()
    } else {
        styled
    };
    styled.animate(Timing::SPRING_QUICK)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            modifiers: Default::default(),
            click_count: 1,
            pointer_kind: None,
            wheel_delta: None,
        }
    }

    #[test]
    fn calendar_day_key_matches_widget_format() {
        assert_eq!(
            calendar_day_key("billing", &"2026-05-13"),
            "billing:day:2026-05-13"
        );
    }

    #[test]
    fn classify_event_routes_nav_and_day_picks() {
        assert_eq!(
            classify_event(&click_event("billing:prev"), "billing"),
            Some(CalendarAction::PreviousMonth),
        );
        assert_eq!(
            classify_event(&click_event("billing:next"), "billing"),
            Some(CalendarAction::NextMonth),
        );
        assert_eq!(
            classify_event(&click_event("billing:day:2026-05-13"), "billing"),
            Some(CalendarAction::Pick("2026-05-13")),
        );
        assert_eq!(
            classify_event(&click_event("billing-extra:day:2026-05-13"), "billing"),
            None,
        );
    }

    #[test]
    fn apply_event_sets_selected_day() {
        let mut selected = None;
        assert!(apply_event(
            &mut selected,
            &click_event("billing:day:2026-05-13"),
            "billing"
        ));
        assert_eq!(selected.as_deref(), Some("2026-05-13"));

        assert!(apply_event(
            &mut selected,
            &click_event("billing:next"),
            "billing"
        ));
        assert_eq!(selected.as_deref(), Some("2026-05-13"));
    }

    #[test]
    fn calendar_month_renders_header_weekdays_and_weeks() {
        let days = (1..=14)
            .map(|d| CalendarDay::new(format!("2026-05-{d:02}"), d.to_string()))
            .collect::<Vec<_>>();
        let cal = calendar_month("billing", "May 2026", days);

        assert_eq!(cal.kind, Kind::Custom("calendar"));
        assert_eq!(cal.children.len(), 3);
        assert_eq!(cal.children[1].children.len(), 7);
        assert_eq!(cal.children[2].children.len(), 2);
        assert_eq!(
            cal.children[2].children[0].children[0].key.as_deref(),
            Some("billing:day:2026-05-01"),
        );
    }

    #[test]
    fn selected_outside_and_disabled_days_change_treatment() {
        let selected = calendar_day(
            Location::caller(),
            "cal",
            &CalendarDay::new("2026-05-13", "13").selected(),
        );
        assert_eq!(selected.fill, Some(tokens::ACCENT));

        let outside = calendar_day(
            Location::caller(),
            "cal",
            &CalendarDay::new("2026-04-30", "30").outside(),
        );
        assert_eq!(outside.text_color, Some(tokens::MUTED_FOREGROUND));

        let disabled = calendar_day(
            Location::caller(),
            "cal",
            &CalendarDay::new("2026-05-14", "14").disabled(),
        );
        assert!(!disabled.focusable);
        assert!(disabled.block_pointer);
    }
}
