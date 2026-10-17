//! Section — a titled region of a tool pane: a header row over a
//! hairline, with slots for a count, a headline value, trailing
//! actions and an optional disclosure, and the body beneath it.
//!
//! # Why this exists
//!
//! Tools group content by **section**, not by card. GitKraken's right
//! pane is one panel with hairline-separated disclosure sections;
//! Linear's and VS Code's inspectors are stacks of titled regions; the
//! kiosk's "section frames" (`references/workbench-validation`,
//! ui-kit-5) were the most tool-like idiom in the whole 2026-10 review
//! set. Apps built on damascene had no such production, so every one
//! of them boxed every region in a `card` and read as a landing page
//! (`docs/POLISH_REVIEW_2026_10_OPUS.md` §4.2, §6.6; ruled 2026-10-14).
//!
//! # Oracle (`docs/NAMING_ORACLE.md`)
//!
//! shadcn has no section component — its answer is `Card`, which is the
//! problem — and Radix has none. The name is the web platform's
//! `<section>` with a heading, and the folding variant is
//! `<details><summary>`, which is what [`collapsible`](crate::widgets::collapsible::collapsible) already models;
//! a collapsible section's header *is* a collapsible trigger with the
//! section's slots, keyed by the app's key verbatim and folded by the
//! same [`apply_event`]. Recorded 2026-10-14.
//!
//! # Shape
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! column([
//!     section_with(
//!         "Live controller",
//!         SectionOpts::default().value("hold · 6.02 kPa").collapsible("controller", self.controller_open),
//!         [knob_rows()],
//!     ),
//!     section_with(
//!         "Zone survey",
//!         SectionOpts::default().count(14).trailing([badge("2 failed").destructive()]),
//!         [zone_table()],
//!     ),
//!     section("Identity", [identity_rows()]),
//! ])
//! ```
//!
//! and in `on_event`, for each folding section,
//! `section::apply_event(&mut self.controller_open, &event, "controller")`.
//!
//! # Anatomy
//!
//! ```text
//! section        column  fill-width  hug  gap=0  border-b=1 BORDER
//! ├── section_header  row  h=36 (default)  gap=SPACE_2  align=center
//! │   ├── text   title   label  FOREGROUND  ellipsis  fill       role=heading level=3
//! │   ├── text   count   caption MUTED_FOREGROUND tnum           (opts.count)
//! │   ├── text   value   caption MUTED_FOREGROUND tnum           (opts.value)
//! │   ├── row    trailing  gap=SPACE_1  controls stamped `Xs`     (opts.trailing)
//! │   └── icon   chevron-down | chevron-right  ICON_SM muted     (opts.collapsible)
//! └── section_body    column  pl=0  pb=SPACE_3  gap=SPACE_2  role=group   (open, or never folds)
//! ```
//!
//! A folding section's header is keyed, focusable, `role=button` with
//! `aria-expanded`; its body is included or dropped per rebuild, with
//! no height animation (shadcn's `Collapsible` likewise). A section
//! that never folds has no chevron and no key: its header is a plain
//! heading and the body is always there.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::a11y::Role;
use crate::anim::Timing;
use crate::cursor::Cursor;
use crate::metrics::ComponentSize;
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::text;
use crate::{IconName, icon};

pub use crate::widgets::collapsible::apply_event;

/// The optional header slots of a [`section_with`].
#[derive(Default)]
pub struct SectionOpts {
    collapse: Option<(String, bool)>,
    count: Option<usize>,
    value: Option<String>,
    trailing: Vec<El>,
    level: u8,
}

impl SectionOpts {
    /// Make the section fold: its header becomes the disclosure
    /// trigger, keyed by `key` verbatim (the [`collapsible`](crate::widgets::collapsible::collapsible) contract),
    /// `open` from the app's state, and the body is shown only while
    /// open. Fold events back with [`apply_event`].
    pub fn collapsible(mut self, key: impl Into<String>, open: bool) -> Self {
        self.collapse = Some((key.into(), open));
        self
    }

    /// A count beside the title — "Staged files (3)", the sidebar
    /// section count — set in the muted caption, tabular.
    pub fn count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }

    /// A headline value at the right edge of the header — the one
    /// measurement the section is about, readable while it is folded
    /// ("hold · 6.02 kPa"). Muted caption, tabular numerals.
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Trailing header controls or badges, right-aligned before the
    /// chevron. Controls without a size of their own are stamped
    /// [`ComponentSize::Xs`], the 28 px rung that fits the 36 px header
    /// with room for the focus ring.
    pub fn trailing<I, E>(mut self, trailing: I) -> Self
    where
        I: IntoIterator<Item = E>,
        E: Into<El>,
    {
        self.trailing = trailing.into_iter().map(Into::into).collect();
        self
    }

    /// The title's `aria-level` (default 3: a pane is a level-2
    /// region, its sections sit under it).
    pub fn level(mut self, level: u8) -> Self {
        self.level = level;
        self
    }
}

/// A section that never folds: a titled header over a hairline and
/// the body beneath it. See [`section_with`] for the header slots.
#[track_caller]
pub fn section<I, E>(title: impl Into<String>, body: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    build(
        Location::caller(),
        title.into(),
        SectionOpts::default(),
        body,
    )
}

/// A section with header slots — a count, a headline value, trailing
/// actions, a disclosure — see [`SectionOpts`] and the module docs.
#[track_caller]
pub fn section_with<I, E>(title: impl Into<String>, opts: SectionOpts, body: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    build(Location::caller(), title.into(), opts, body)
}

/// The body column of a section — the slot [`section`] puts its
/// children in. Public so a hand-built header can pair with the stock
/// body spacing. The body's left edge is the title's: no inset, so a
/// pane of stacked sections keeps one left rule for titles, rows and
/// fields alike (ruled 2026-10-16 after a blind round measured four
/// different left edges down one pane).
#[track_caller]
pub fn section_body<I, E>(children: I) -> El
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
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: tokens::SPACE_3,
        })
        .gap(tokens::SPACE_2)
}

fn build<I, E>(caller: &'static Location<'static>, title: String, opts: SectionOpts, body: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let level = if opts.level == 0 { 3 } else { opts.level };
    let open = opts.collapse.as_ref().map(|(_, open)| *open);

    let mut header_children: Vec<El> = Vec::with_capacity(5);
    header_children.push(
        text(title.clone())
            .label()
            .ellipsis()
            .width(Size::Fill(1.0))
            .role(Role::Heading)
            .aria_level(level),
    );
    if let Some(count) = opts.count {
        header_children.push(text(count.to_string()).caption().tabular_numerals());
    }
    if let Some(value) = opts.value {
        header_children.push(text(value).caption().tabular_numerals());
    }
    if !opts.trailing.is_empty() {
        let mut trailing = opts.trailing;
        for control in &mut trailing {
            if control.metrics_role.is_some() && control.component_size.is_none() {
                control.component_size = Some(ComponentSize::Xs);
            }
        }
        header_children.push(
            row(trailing)
                .gap(tokens::SPACE_1)
                .align(Align::Center)
                .width(Size::Hug)
                .height(Size::Hug),
        );
    }
    if let Some(open) = open {
        header_children.push(
            icon(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .default_icon_size(tokens::ICON_SM)
            .color(tokens::MUTED_FOREGROUND),
        );
    }

    let mut header = row(header_children)
        .gap(tokens::SPACE_2)
        .align(Align::Center)
        .width(Size::Fill(1.0))
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT));
    header.kind = Kind::Custom("section_header");
    if let Some((key, open)) = &opts.collapse {
        header = header
            .key(key.clone())
            .role(Role::Button)
            .aria_expanded(*open)
            .focusable()
            .focus_ring_inside()
            .cursor(Cursor::Pointer)
            // A radius so the ghost hover wash has a shape to fade in.
            .default_radius(tokens::RADIUS_SM)
            .animate(Timing::SPRING_QUICK);
    }

    let mut children = vec![header];
    if open.unwrap_or(true) {
        children.push(section_body(body));
    }
    let mut el = column(children)
        .at_loc(caller)
        .role(Role::Group)
        .aria_label(title)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .gap(0.0)
        .border_b();
    el.kind = Kind::Custom("section");
    el
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::lint::{FindingKind, lint};
    use crate::event::{KeyModifiers, UiEvent, UiEventKind};
    use crate::layout::layout;
    use crate::state::UiState;
    use crate::widgets::badge::badge;
    use crate::widgets::button::icon_button;

    fn header(s: &El) -> &El {
        &s.children[0]
    }

    #[test]
    fn section_has_the_documented_anatomy() {
        let s = section("Identity", [text("a group that never folds")]);
        assert_eq!(s.kind, Kind::Custom("section"));
        assert_eq!(s.axis, Axis::Column);
        assert_eq!(s.width, Size::Fill(1.0));
        assert_eq!(s.gap, 0.0);
        let rule = s.border.as_deref().expect("hairline under the frame");
        assert_eq!(rule.widths, Sides::bottom(1.0));
        assert_eq!(
            s.a11y.as_deref().and_then(|a| a.label.as_deref()),
            Some("Identity")
        );

        let h = header(&s);
        assert_eq!(h.kind, Kind::Custom("section_header"));
        assert_eq!(h.height, Size::Fixed(tokens::CONTROL_HEIGHT));
        assert!(
            h.key.is_none() && !h.focusable,
            "a section that never folds is not a control"
        );
        let title = &h.children[0];
        assert_eq!(title.text.as_deref(), Some("Identity"));
        assert_eq!(title.text_role, TextRole::Label);
        assert_eq!(title.width, Size::Fill(1.0));
        assert_eq!(
            title.a11y.as_deref().and_then(|a| a.role),
            Some(Role::Heading)
        );
        assert_eq!(title.a11y.as_deref().and_then(|a| a.level), Some(3));
        assert_eq!(
            h.children.len(),
            1,
            "no chevron on a section that never folds"
        );

        let body = &s.children[1];
        assert_eq!(
            body.padding.left, 0.0,
            "the body shares the title's left edge"
        );
        assert_eq!(body.padding.bottom, tokens::SPACE_3);
        assert_eq!(body.a11y.as_deref().and_then(|a| a.role), Some(Role::Group));
    }

    #[test]
    fn header_slots_land_in_order_and_trailing_controls_take_the_xs_rung() {
        let s = section_with(
            "Zone survey",
            SectionOpts::default()
                .count(14)
                .value("hold · 6.02 kPa")
                .trailing([badge("2 failed").destructive(), icon_button("eye").ghost()]),
            [text("rows")],
        );
        let h = header(&s);
        let [title, count, value, trailing] = &h.children[..] else {
            panic!("title, count, value, trailing: {}", h.children.len());
        };
        assert_eq!(title.text.as_deref(), Some("Zone survey"));
        assert_eq!(count.text.as_deref(), Some("14"));
        assert_eq!(count.text_role, TextRole::Caption);
        assert_eq!(value.text.as_deref(), Some("hold · 6.02 kPa"));
        assert_eq!(trailing.children.len(), 2);
        assert_eq!(
            trailing.children[1].component_size,
            Some(ComponentSize::Xs),
            "an unsized control is stamped to the header's rung"
        );
        assert_eq!(
            trailing.children[0].component_size,
            Some(ComponentSize::Xs),
            "a badge is a sized control too"
        );
    }

    #[test]
    fn a_folding_section_is_a_disclosure_keyed_by_the_app_key() {
        let open = section_with(
            "Setpoint",
            SectionOpts::default().collapsible("setpoint", true),
            [text("the section's editors live here")],
        );
        let h = header(&open);
        assert_eq!(h.key.as_deref(), Some("setpoint"));
        assert!(h.focusable);
        assert_eq!(h.a11y.as_deref().and_then(|a| a.role), Some(Role::Button));
        assert_eq!(h.a11y.as_deref().and_then(|a| a.expanded), Some(true));
        let chevron = h.children.last().unwrap();
        assert_eq!(
            chevron.icon,
            Some(crate::IconSource::Builtin(IconName::ChevronDown))
        );
        assert_eq!(open.children.len(), 2, "open: body present");

        let closed = section_with(
            "Setpoint",
            SectionOpts::default().collapsible("setpoint", false),
            [text("the section's editors live here")],
        );
        let h = header(&closed);
        assert_eq!(h.a11y.as_deref().and_then(|a| a.expanded), Some(false));
        let chevron = h.children.last().unwrap();
        assert_eq!(
            chevron.icon,
            Some(crate::IconSource::Builtin(IconName::ChevronRight))
        );
        assert_eq!(closed.children.len(), 1, "closed: body dropped");

        // The collapsible contract folds it.
        let mut state = false;
        let click = UiEvent {
            path: None,
            kind: UiEventKind::Click,
            key: Some("setpoint".into()),
            target: None,
            pointer: None,
            key_press: None,
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 1,
            pointer_kind: None,
            wheel_delta: None,
        };
        assert!(apply_event(&mut state, &click, "setpoint"));
        assert!(state);
    }

    #[test]
    fn a_stack_of_sections_is_lint_clean() {
        let mut root = crate::page([column([
            section_with(
                "Setpoint",
                SectionOpts::default().collapsible("setpoint", true),
                [text("the section's editors live here")],
            ),
            section_with(
                "Live controller",
                SectionOpts::default()
                    .value("hold · 6.02 kPa")
                    .collapsible("controller", false),
                [text("knobs")],
            ),
            section_with(
                "Zone survey",
                SectionOpts::default().count(14).trailing([
                    badge("2 failed").destructive(),
                    icon_button("eye").ghost().aria_label("Show"),
                ]),
                [text("table")],
            ),
            section("Identity", [text("a group that never folds")]),
        ])
        .align(Align::Stretch)]);
        // Hosts run the metrics pass before layout; it is what turns
        // the `Xs` stamp on the trailing controls into their 28 px box.
        let theme = crate::theme::Theme::default();
        theme.apply_metrics(&mut root);
        let mut ui_state = UiState::new();
        layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 480.0, 400.0));
        let report = lint(&root, &ui_state, &theme);
        let noisy: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.kind != FindingKind::UnpaddedViewportLeaf)
            .collect();
        assert!(noisy.is_empty(), "{}", report.text());
    }
}
