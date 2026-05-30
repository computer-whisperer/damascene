//! Form — shadcn-shaped vertical form anatomy plus compact field rows.
//!
//! The boring path mirrors the common web component shape:
//! `form([form_item([form_label(...), form_control(...), form_description(...)])])`.
//! `field_row` remains the compact horizontal variant for settings
//! rows, preference panes, and audio-config modals.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct Prefs { auto_save: bool, volume: f32 }
//!
//! impl App for Prefs {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         card([
//!             card_header([card_title("Audio")]),
//!             card_content([form([
//!                 form_item([
//!                     form_label("Preset"),
//!                     form_control(text_input("Studio", &Selection::default(), "preset")),
//!                     form_description("Used for new sessions."),
//!                 ]),
//!                 field_row("Auto-save", switch(self.auto_save).key("auto_save")),
//!             ])]),
//!         ])
//!     }
//! }
//! ```
//!
//! # Dogfood note
//!
//! Pure composition over the public widget-kit surface — `row`,
//! `spacer`, [`crate::widgets::text::text`] with the `.label()` role
//! modifier. No internal machinery; an app crate can fork this file
//! and produce an equivalent helper.

use std::panic::Location;

use super::text::text;
use crate::metrics::MetricsRole;
use crate::tokens;
use crate::tree::*;

#[track_caller]
pub fn form<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    // Horizontal padding equal to RING_WIDTH so focusable controls
    // sitting flush at the form's edges still have room to paint
    // their focus ring inside any clipping ancestor (e.g., the
    // surrounding `scroll(...)`). Without this, the ring's
    // `paint_overflow` band falls outside the ancestor's scissor and
    // gets cut on the L/R edges.
    column(children)
        .at_loc(Location::caller())
        .metrics_role(MetricsRole::Form)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_gap(tokens::SPACE_3)
        .default_padding(Sides::xy(tokens::RING_WIDTH, 0.0))
}

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
        .height(Size::Hug)
        .default_gap(tokens::SPACE_3)
}

#[track_caller]
pub fn form_item<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .metrics_role(MetricsRole::FormItem)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_gap(tokens::SPACE_2)
}

#[track_caller]
pub fn form_label(label: impl Into<String>) -> El {
    text(label)
        .at_loc(Location::caller())
        .label()
        .ellipsis()
        .width(Size::Fill(1.0))
}

#[track_caller]
pub fn form_control(control: impl Into<El>) -> El {
    El::new(Kind::Custom("form_control"))
        .at_loc(Location::caller())
        .child(control)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

#[track_caller]
pub fn form_description(description: impl Into<String>) -> El {
    text(description)
        .at_loc(Location::caller())
        .muted()
        .wrap_text()
        .fill_width()
}

#[track_caller]
pub fn form_message(message: impl Into<String>) -> El {
    text(message)
        .at_loc(Location::caller())
        .font_weight(FontWeight::Medium)
        .destructive()
        .wrap_text()
        .fill_width()
}

/// A labelled form row: label on the left, control on the right,
/// vertical-center aligned, full panel width.
///
/// The label is styled with the `.label()` text role
/// ([`TextRole::Label`]) so it picks up the same size, weight, and
/// theme color as standalone form labels (next to checkboxes,
/// switches, etc.). The control is any `El` — a switch, a slider,
/// a button, a row of controls, anything that fits on the right.
///
/// For multi-control rows (e.g. a value readout next to a slider),
/// wrap them in a `row([...])` and pass that as `control`.
#[track_caller]
pub fn field_row(label: impl Into<String>, control: impl Into<El>) -> El {
    crate::row([text(label).label(), crate::spacer(), control.into()])
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
    fn form_message_uses_error_treatment() {
        let message = form_message("Email is required.");

        assert_eq!(message.text_role, TextRole::Body);
        assert_eq!(message.font_size, tokens::TEXT_SM.size);
        assert_eq!(message.line_height, tokens::TEXT_SM.line_height);
        assert_eq!(message.font_weight, FontWeight::Medium);
        assert_eq!(message.text_color, Some(tokens::DESTRUCTIVE));
    }

    #[test]
    fn field_row_lays_out_label_spacer_control() {
        // The fixed shape — label first, spacer second, control last
        // — is what gives every form row the same visual rhythm.
        // Apps reading routed events through `target_key` rely on the
        // control keeping its own key, so the wrapper must not
        // interpose its own.
        let r = field_row("Auto-save", switch(false).key("auto_save"));
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
        let r = field_row("Theme", switch(true).key("theme"));
        assert!(matches!(r.width, Size::Fill(_)));
        assert_eq!(r.align, Align::Center);
    }

    #[test]
    fn field_row_accepts_dynamic_label() {
        // Apps frequently format the label with the current value
        // (e.g. "Volume (52%)"). String types must satisfy
        // `Into<String>` the same way `card` titles do.
        let r = field_row(format!("Volume ({}%)", 52), switch(false).key("k"));
        let label = &r.children[0];
        assert_eq!(label.text.as_deref(), Some("Volume (52%)"));
    }
}
