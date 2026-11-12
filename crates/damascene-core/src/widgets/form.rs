//! Form — HTML's `<form>` with shadcn's field anatomy: a keyed
//! container that is submitted as a whole, plus compact field rows.
//!
//! The boring path mirrors the common web component shape:
//! `form(key, [form_item([form_label(...), form_control(...), form_description(...)])])`.
//!
//! # Submission
//!
//! A form is submitted the way an HTML one is: by its submit button
//! (any control marked [`.submit()`](crate::tree::El::submit)) or by
//! plain Enter in one of its single-line text fields. Either way the
//! app receives one [`UiEventKind::Submit`] routed to the form's key,
//! and [`submitted`] is the check — the form's `onsubmit`. Fields
//! added later submit without the handler changing. While the submit
//! button is `.disabled()`, Enter submits nothing either.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct SignIn { email: String, password: String, selection: Selection }
//!
//! impl App for SignIn {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         form("sign-in", [
//!             form_item([
//!                 form_label("Email"),
//!                 form_control(text_input("email", &self.email, &self.selection)),
//!             ]),
//!             form_item([
//!                 form_label("Password"),
//!                 form_control(text_input_with(
//!                     "password",
//!                     &self.password,
//!                     &self.selection,
//!                     TextInputOpts::default().password(),
//!                 )),
//!             ]),
//!             button("Sign in").key("sign-in:go").primary().submit(),
//!         ])
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
//!         text_input::apply_event(&mut self.email, &mut self.selection, &event, "email");
//!         text_input::apply_event(&mut self.password, &mut self.selection, &event, "password");
//!         if form::submitted(&event, "sign-in") {
//!             self.sign_in();
//!         }
//!     }
//! }
//! ```
//!
//! The button's own `Click` and the field's own `KeyDown` still
//! arrive, just before the `Submit`; handle the `Submit` and leave
//! those alone. A lone field with no form around it (a search box, an
//! add-on-Enter row) keeps using
//! [`text_input::submitted`](crate::widgets::text_input::submitted).
//!
//! # Settings rows
//!
//! For settings rows and inspector panes, prefer the
//! [`crate::widgets::field`] anatomy (`field` / `field_with` /
//! `field_group` / `field_set`) — it is the shadcn `Field` shape those
//! panes are built from, and `FieldOpts` carries the description
//! slot, the trailing-control width, and the label gutter that
//! [`field_row`] deliberately doesn't. `field_row` stays as the bare
//! label-spacer-control primitive. A pane of settings that apply as
//! they change has nothing to submit and wants `field_group`, not a
//! form.
//!
//! # Dogfood note
//!
//! Pure composition over the public widget-kit surface — `column`,
//! `row`, `spacer`, [`crate::widgets::text::text`] with the `.label()`
//! role modifier, and the public
//! [`.form_owner()`](crate::tree::El::form_owner) marker, which is
//! all that makes a container submittable. An app crate can fork this
//! file and produce an equivalent helper.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use super::text::text;
use crate::event::{UiEvent, UiEventKind};
use crate::metrics::MetricsRole;
use crate::tokens;
use crate::tree::*;

/// Vertical form container (HTML `<form>`, shadcn's `Form`) — a
/// full-width column of [`form_item`]s / [`form_section`]s, keyed so
/// it can be submitted: its submit button and Enter in its single-line
/// text fields deliver a [`UiEventKind::Submit`] routed to `key`,
/// which [`submitted`] matches (see the [module docs](self)).
///
/// No inset of its own: inside a `scroll` the controls' focus rings
/// paint in the scroll's clip margin (see [`El::clip_margin`]).
#[track_caller]
pub fn form<I, E>(key: impl Into<String>, children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .key(key)
        .form_owner()
        // Keyed for routing only: the form is not a control, so the
        // pointer leaves no hover or press state on it.
        .no_hover()
        .metrics_role(MetricsRole::Form)
        .width(Size::Fill(1.0))
        .default_gap(tokens::SPACE_3)
}

/// True when `event` submits the form keyed `key` — HTML's `onsubmit`,
/// the one check an `on_event` makes for the whole form.
///
/// The runtime delivers the [`UiEventKind::Submit`] it matches after
/// a click or keyboard activation of a
/// [`.submit()`](El::submit) control inside the form, and after plain
/// Enter in one of its single-line text fields (see
/// [`El::form_owner`] for the full rule). Tests build one with
/// [`UiEvent::synthetic_submit`].
///
/// ```
/// use damascene_core::event::UiEvent;
/// use damascene_core::widgets::form;
///
/// assert!(form::submitted(&UiEvent::synthetic_submit("sign-in"), "sign-in"));
/// // The button that caused it is a different event.
/// assert!(!form::submitted(&UiEvent::synthetic_click("sign-in:go"), "sign-in"));
/// ```
pub fn submitted(event: &UiEvent, key: &str) -> bool {
    event.kind == UiEventKind::Submit && event.is_route(key)
}

/// Grouping of related [`form_item`]s inside a [`form`] — same column
/// rhythm, without the form's edge padding.
#[track_caller]
pub fn form_section<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .metrics_role(MetricsRole::Form)
        .width(Size::Fill(1.0))
        .default_gap(tokens::SPACE_3)
}

/// One field's vertical stack (shadcn's `FormItem`) — typically
/// [`form_label`], [`form_control`], then [`form_description`] or
/// [`form_message`].
///
/// The label is wired to the control the way HTML's implicit `<label>`
/// association works: the first label-role text child names the first
/// focusable node inside the first [`form_control`] child, unless that
/// node already carries an explicit `aria_label`.
#[track_caller]
pub fn form_item<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut children: Vec<El> = children.into_iter().map(Into::into).collect();
    let label = children
        .iter()
        .find(|c| c.text_role == TextRole::Label)
        .and_then(|c| c.text.clone());
    if let Some(label) = label
        && let Some(control) = children
            .iter_mut()
            .find(|c| c.kind == Kind::Custom("form_control"))
    {
        crate::a11y::associate_label(control, &label);
    }
    column(children)
        .at_loc(Location::caller())
        .metrics_role(MetricsRole::FormItem)
        .width(Size::Fill(1.0))
        .default_gap(tokens::SPACE_2)
}

/// Field label above the control (HTML `<label>`, shadcn's
/// `FormLabel`).
#[track_caller]
pub fn form_label(label: impl Into<String>) -> El {
    text(label)
        .at_loc(Location::caller())
        .label()
        .ellipsis()
        .width(Size::Fill(1.0))
}

/// Full-width wrapper around the field's input control (shadcn's
/// `FormControl`).
#[track_caller]
pub fn form_control(control: impl Into<El>) -> El {
    // Column, not the bare-`El` Overlay default: overlay containers
    // cap Hug children at their intrinsic size and never stretch
    // them, which silently collapsed a bare `row([...])` wrapper
    // around a Fill-width control (input + trailing badge, issue
    // #120). A column's cross-axis stretch gives such wrappers the
    // full control width.
    El::new(Kind::Custom("form_control"))
        .at_loc(Location::caller())
        .child(control)
        .axis(Axis::Column)
        .width(Size::Fill(1.0))
}

/// Muted helper text under the control (shadcn's `FormDescription`).
#[track_caller]
pub fn form_description(description: impl Into<String>) -> El {
    text(description)
        .at_loc(Location::caller())
        .muted()
        .wrap_text()
        .fill_width()
}

/// Destructive-tinted validation message under the control (shadcn's
/// `FormMessage`).
#[track_caller]
pub fn form_message(message: impl Into<String>) -> El {
    text(message)
        .at_loc(Location::caller())
        .font_weight(FontWeight::Medium)
        .destructive()
        .wrap_text()
        .fill_width()
}

/// The bare label-spacer-control primitive: label on the left, control
/// pushed to the right by a [`crate::spacer`], vertical-center
/// aligned, full panel width.
///
/// **This is not the settings-row helper — reach for
/// [`crate::widgets::field::field_with`] first.** `field_row` is
/// deliberately minimal: no description slot, no control sizing, no
/// label gutter. The [`crate::widgets::field`] anatomy covers the two
/// shapes panels and inspectors actually want, and both accept a
/// description line:
///
/// - settings row — `FieldOpts::default().horizontal()`, plus
///   `.control_width(w)` to land a page of trailing selects on one
///   right edge (a `Fill`-width control in a bare `field_row` splits
///   the row 50/50 instead);
/// - inspector row — `FieldOpts::default().inline_label(w)`, a fixed
///   label gutter with the control filling the rest.
///
/// What's left for `field_row`: a one-off row whose control is
/// intrinsically sized (a switch, a small button, a
/// `row([...])` of controls) and which wants nothing from `FieldOpts`.
///
/// The label is styled with the `.label()` text role
/// ([`TextRole::Label`]) so it picks up the same size, weight, and
/// theme color as standalone form labels (next to checkboxes,
/// switches, etc.).
#[track_caller]
pub fn field_row(label: impl Into<String>, control: impl Into<El>) -> El {
    let label = label.into();
    let mut control = control.into();
    // Same implicit label association as `form_item` — the row's
    // label names the control for assistive technology.
    crate::a11y::associate_label(&mut control, &label);
    crate::row([text(label).label(), crate::spacer(), control])
        .at_loc(Location::caller())
        .gap(tokens::SPACE_3)
        .align(Align::Center)
        .width(Size::Fill(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::switch::switch;

    #[test]
    fn form_item_lays_out_label_control_and_description() {
        let item = form_item([
            form_label("Email"),
            form_control("alicia@example.com"),
            form_description("Used for notifications."),
        ]);

        assert_eq!(item.metrics_role, Some(MetricsRole::FormItem));
        assert_eq!(item.axis, Axis::Column);
        assert_eq!(item.children.len(), 3);
        assert_eq!(item.children[0].text.as_deref(), Some("Email"));
        assert_eq!(item.children[0].text_role, TextRole::Label);
        assert_eq!(item.children[1].kind, Kind::Custom("form_control"));
        assert_eq!(item.children[2].text_role, TextRole::Body);
        assert_eq!(item.children[2].font_size, tokens::TEXT_SM.size);
        assert_eq!(item.children[2].line_height, tokens::TEXT_SM.line_height);
        assert_eq!(item.children[2].text_color, Some(tokens::MUTED_FOREGROUND));
    }

    #[test]
    fn form_control_stretches_bare_hug_row_wrappers_issue_120() {
        // Issue #120's repro: a Fill-width input paired with a
        // trailing badge in a bare `row([...])` (Hug width) inside
        // `form_control`. With the old Overlay-axis wrapper the Hug
        // row capped at its intrinsic and the input collapsed to its
        // padding (~20px); the column wrapper's cross-axis stretch
        // must hand the row the full control width.
        let selection = crate::Selection::default();
        let mut root = form_control(
            crate::row([
                crate::text_input("url", "http://127.0.0.1:8188", &selection),
                text("reachable"),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
        );
        let mut state = crate::UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 600.0, 48.0));

        let row = &root.children[0];
        assert!(
            (row.computed_rect.w - 600.0).abs() < 0.5,
            "wrapper row should stretch to the control width, got {}",
            row.computed_rect.w
        );
        let input = &row.children[0];
        assert!(
            input.computed_rect.w > 400.0,
            "Fill input should claim the row's slack, got {}",
            input.computed_rect.w
        );
    }

    #[test]
    fn form_message_uses_error_treatment() {
        let message = form_message("Email is required.");

        assert_eq!(message.text_role, TextRole::Body);
        assert_eq!(message.font_size, tokens::TEXT_SM.size);
        assert_eq!(message.line_height, tokens::TEXT_SM.line_height);
        assert_eq!(message.font_weight, FontWeight::Medium);
        assert_eq!(
            message.text_color,
            Some(tokens::DESTRUCTIVE_TINT_FOREGROUND),
            "validation text uses the text-grade destructive tone"
        );
    }

    #[test]
    fn form_item_label_names_the_control_like_html_label_for() {
        let item = form_item([
            form_label("Email"),
            form_control(crate::text_input(
                "email",
                "a@example.com",
                &crate::Selection::default(),
            )),
        ]);
        let control = &item.children[1];
        let input = &control.children[0];
        assert!(input.focusable, "text_input is the focusable node");
        assert_eq!(
            input.a11y.as_deref().and_then(|p| p.label.as_deref()),
            Some("Email"),
            "implicit label association names the input"
        );

        // An explicit aria_label on the control wins, as in HTML.
        let item = form_item([
            form_label("Email"),
            form_control(
                crate::text_input("email", "a@example.com", &crate::Selection::default())
                    .aria_label("Work email"),
            ),
        ]);
        let input = &item.children[1].children[0];
        assert_eq!(
            input.a11y.as_deref().and_then(|p| p.label.as_deref()),
            Some("Work email")
        );
    }

    #[test]
    fn field_row_label_names_the_control() {
        let row = field_row("Auto-save", switch("auto_save", true));
        let control = &row.children[2];
        assert!(control.focusable);
        assert_eq!(
            control.a11y.as_deref().and_then(|p| p.label.as_deref()),
            Some("Auto-save")
        );
    }

    #[test]
    fn field_row_lays_out_label_spacer_control() {
        // The fixed shape — label first, spacer second, control last
        // — is what gives every form row the same visual rhythm.
        // Apps reading routed events through `target_key` rely on the
        // control keeping its own key, so the wrapper must not
        // interpose its own.
        let r = field_row("Auto-save", switch("auto_save", false));
        assert_eq!(r.children.len(), 3);
        assert_eq!(r.axis, Axis::Row);
        assert!(r.key.is_none(), "field_row carries no key of its own");

        let label = &r.children[0];
        assert_eq!(label.text.as_deref(), Some("Auto-save"));
        assert_eq!(label.text_role, TextRole::Label);

        let spacer = &r.children[1];
        assert_eq!(spacer.kind, Kind::Spacer);

        let control = &r.children[2];
        assert_eq!(control.key.as_deref(), Some("auto_save"));
    }

    #[test]
    fn field_row_fills_width_and_centers_vertically() {
        // The row hugs its parent's width so a column of field rows
        // forms a clean stack; centered alignment lets a tall control
        // (a paragraph of helper text inside the control slot, say)
        // sit beside a single-line label without baseline drift.
        let r = field_row("Theme", switch("theme", true));
        assert!(matches!(r.width, Size::Fill(_)));
        assert_eq!(r.align, Align::Center);
    }

    #[test]
    fn field_row_accepts_dynamic_label() {
        // Apps frequently format the label with the current value
        // (e.g. "Volume (52%)"). String types must satisfy
        // `Into<String>` the same way `card` titles do.
        let r = field_row(format!("Volume ({}%)", 52), switch("k", false));
        let label = &r.children[0];
        assert_eq!(label.text.as_deref(), Some("Volume (52%)"));
    }
}
