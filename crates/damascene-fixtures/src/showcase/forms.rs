//! Forms — composition pattern.
//!
//! Demonstrates the `form` / `form_section` / `form_item` /
//! `form_label` / `form_control` / `form_description` / `form_message`
//! anatomy. Each anatomy is what you reach for when a single label needs
//! to live above its control with helper text below and an
//! validation/error message slot ready.

use damascene_core::prelude::*;

pub struct State {
    pub display_name: String,
    pub email: String,
    pub bio: String,
    pub billing_date: Option<String>,
    pub billing_month: YearMonth,
    pub selection: Selection,
    pub show_errors: bool,
    pub scroll_bio_caret_into_view: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YearMonth {
    year: i32,
    month: u8,
}

impl YearMonth {
    const fn new(year: i32, month: u8) -> Self {
        Self { year, month }
    }

    fn previous(self) -> Self {
        if self.month == 1 {
            Self::new(self.year - 1, 12)
        } else {
            Self::new(self.year, self.month - 1)
        }
    }

    fn next(self) -> Self {
        if self.month == 12 {
            Self::new(self.year + 1, 1)
        } else {
            Self::new(self.year, self.month + 1)
        }
    }
}

impl Default for YearMonth {
    fn default() -> Self {
        Self::new(2026, 5)
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            display_name: "Christian".into(),
            email: "user@".into(),
            bio: "Building Damascene — a renderer-agnostic UI kit for Rust apps and AI agents."
                .into(),
            billing_date: Some("2026-05-13".into()),
            billing_month: YearMonth::default(),
            selection: Selection::default(),
            show_errors: true,
            scroll_bio_caret_into_view: false,
        }
    }
}

pub fn view(state: &State) -> El {
    let email_error = state
        .show_errors
        .then(|| email_validation_message(&state.email))
        .flatten();

    scroll([form([
        h1("Forms"),
        paragraph(
            "Composition pattern: each `form_item` pairs a `form_label`, \
             `form_control`, optional `form_description`, and an \
             error/validation `form_message` slot. The widget wraps a \
             plain text/select/etc. control — apps remain in charge of \
             value capture.",
        )
        .muted(),
        section_with_heading(
            "Profile",
            "Public-facing identity",
            form_section([
                form_item([
                    form_label("Display name"),
                    form_control(text_input(
                        "forms-display-name",
                        &state.display_name,
                        &state.selection,
                    )),
                    form_description("Shown next to comments and pull requests."),
                ]),
                form_item({
                    let mut parts = vec![
                        form_label("Email"),
                        form_control(text_input("forms-email", &state.email, &state.selection)),
                        form_description("We'll send password resets here."),
                    ];
                    if let Some(msg) = email_error.as_deref() {
                        parts.push(form_message(msg));
                    }
                    parts
                }),
            ]),
        ),
        section_with_heading(
            "About",
            "Free-form bio shown on your profile",
            form_section([form_item([
                form_label("Bio"),
                form_control(
                    text_area("forms-bio", &state.bio, &state.selection).height(Size::Fixed(96.0)),
                ),
                form_description("Markdown-style bold and italic render in your profile card."),
            ])]),
        ),
        section_with_heading(
            "Billing",
            "Date picker anatomy",
            form_section([form_item([
                form_label("Renewal date"),
                form_control(calendar_month(
                    "forms-billing-date",
                    month_label(state.billing_month),
                    calendar_days(state.billing_month, state.billing_date.as_deref()),
                )),
                form_description(format!(
                    "Selected date: {}",
                    state.billing_date.as_deref().unwrap_or("none")
                )),
            ])]),
        ),
        row([
            spacer(),
            button("Cancel").ghost().key("forms-cancel"),
            button("Save").primary().key("forms-save"),
        ])
        .gap(tokens::SPACE_2),
    ])
    .gap(tokens::SPACE_4)
    .padding(Sides {
        left: tokens::RING_WIDTH,
        right: tokens::SCROLLBAR_HITBOX_WIDTH,
        top: 0.0,
        bottom: 0.0,
    })])
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, e: UiEvent) {
    if let Some(action) = calendar::classify_event(&e, "forms-billing-date") {
        match action {
            CalendarAction::PreviousMonth => state.billing_month = state.billing_month.previous(),
            CalendarAction::NextMonth => state.billing_month = state.billing_month.next(),
            CalendarAction::Pick(value) => state.billing_date = Some(value.to_string()),
            _ => {}
        }
        return;
    }
    match e.target_key() {
        Some("forms-display-name") => {
            text_input::apply_event(
                &mut state.display_name,
                &mut state.selection,
                "forms-display-name",
                &e,
            );
        }
        Some("forms-email") => {
            text_input::apply_event(&mut state.email, &mut state.selection, "forms-email", &e);
        }
        Some("forms-bio")
            if text_area::apply_event(&mut state.bio, &mut state.selection, "forms-bio", &e) =>
        {
            state.scroll_bio_caret_into_view = true;
        }
        _ => {}
    }
}

fn calendar_days(month: YearMonth, selected: Option<&str>) -> Vec<CalendarDay> {
    let mut days = Vec::with_capacity(42);
    let previous = month.previous();
    let next = month.next();
    let leading = first_weekday_sunday(month);
    let current_days = days_in_month(month);
    let previous_days = days_in_month(previous);

    for i in 0..leading {
        let day = previous_days - leading as u8 + i as u8 + 1;
        days.push(outside_day(previous, day));
    }

    for day in 1..=current_days {
        let value = date_value(month, day);
        let mut cell = CalendarDay::new(value.clone(), day.to_string());
        if selected == Some(value.as_str()) {
            cell = cell.selected();
        }
        days.push(cell);
    }

    let mut day = 1;
    while days.len() < 42 {
        days.push(outside_day(next, day));
        day += 1;
    }
    days
}

fn outside_day(month: YearMonth, day: u8) -> CalendarDay {
    CalendarDay::new(date_value(month, day), day.to_string())
        .outside()
        .disabled()
}

fn date_value(month: YearMonth, day: u8) -> String {
    format!("{}-{:02}-{day:02}", month.year, month.month)
}

fn month_label(month: YearMonth) -> String {
    format!("{} {}", month_name(month.month), month.year)
}

fn month_name(month: u8) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "Unknown",
    }
}

fn days_in_month(month: YearMonth) -> u8 {
    match month.month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(month.year) => 29,
        2 => 28,
        _ => 30,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn first_weekday_sunday(month: YearMonth) -> usize {
    // Sakamoto's algorithm: 0 = Sunday, 1 = Monday, ...
    const OFFSETS: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut year = month.year;
    if month.month < 3 {
        year -= 1;
    }
    ((year + year / 4 - year / 100 + year / 400 + OFFSETS[month.month as usize - 1] + 1) % 7)
        as usize
}

pub fn drain_scroll_requests(state: &mut State) -> Vec<damascene_core::scroll::ScrollRequest> {
    if std::mem::take(&mut state.scroll_bio_caret_into_view)
        && let Some(req) =
            text_area::caret_scroll_request_for(&state.bio, &state.selection, "forms-bio")
    {
        vec![req]
    } else {
        Vec::new()
    }
}

fn section_with_heading(title: &str, subtitle: &str, body: El) -> El {
    column([
        h3(title.to_string()),
        text(subtitle.to_string()).muted().small(),
        body,
    ])
    .gap(tokens::SPACE_2)
    .width(Size::Fill(1.0))
}

/// Toy email validation. Returns `Some(message)` when the value is
/// not yet acceptable; `None` when it parses cleanly.
fn email_validation_message(s: &str) -> Option<String> {
    if s.is_empty() {
        return Some("Email is required.".into());
    }
    if !s.contains('@') {
        return Some("Email must contain `@`.".into());
    }
    let parts: Vec<&str> = s.splitn(2, '@').collect();
    if parts[1].is_empty() {
        return Some("Domain is required after `@`.".into());
    }
    if !parts[1].contains('.') {
        return Some("Domain looks incomplete.".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_validation_flags_missing_at() {
        assert!(email_validation_message("hello").is_some());
        assert!(email_validation_message("hello@example.com").is_none());
        assert!(email_validation_message("").is_some());
        assert!(email_validation_message("hello@").is_some());
        assert!(email_validation_message("hello@nodot").is_some());
    }

    #[test]
    fn calendar_pick_updates_billing_date() {
        let mut state = State::default();
        on_event(
            &mut state,
            UiEvent::synthetic_click("forms-billing-date:day:2026-05-21"),
        );
        assert_eq!(state.billing_date.as_deref(), Some("2026-05-21"));
    }

    #[test]
    fn calendar_prev_next_change_visible_month() {
        let mut state = State::default();
        assert_eq!(state.billing_month, YearMonth::new(2026, 5));

        on_event(
            &mut state,
            UiEvent::synthetic_click("forms-billing-date:next"),
        );
        assert_eq!(state.billing_month, YearMonth::new(2026, 6));

        on_event(
            &mut state,
            UiEvent::synthetic_click("forms-billing-date:prev"),
        );
        on_event(
            &mut state,
            UiEvent::synthetic_click("forms-billing-date:prev"),
        );
        assert_eq!(state.billing_month, YearMonth::new(2026, 4));
    }

    #[test]
    fn calendar_days_follow_visible_month() {
        assert_eq!(month_label(YearMonth::new(2026, 6)), "June 2026");
        let days = calendar_days(YearMonth::new(2026, 6), Some("2026-06-15"));
        assert_eq!(days.len(), 42);
        assert_eq!(days[0].value, "2026-05-31");
        assert!(days[0].outside);
        assert_eq!(days[1].value, "2026-06-01");
        assert!(days.iter().any(|d| d.value == "2026-06-15" && d.selected));
    }
}
