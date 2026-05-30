//! Text constructors.
//!
//! [`text`] is default body text. [`h1`] / [`h2`] / [`h3`] are heading
//! constructors used directly.
//!
//! Modifiers (`.muted`, `.bold`, `.semibold`, `.small`, `.color`, etc.)
//! are inherent methods on [`El`]; see [`crate::style`].

use std::panic::Location;

use crate::style::StyleProfile;
use crate::tree::*;

#[track_caller]
pub fn text(s: impl Into<String>) -> El {
    El::new(Kind::Text)
        .at_loc(Location::caller())
        .style_profile(StyleProfile::TextOnly)
        .text(s)
        .text_role(TextRole::Body)
        .hug()
}

#[track_caller]
pub fn paragraph(s: impl Into<String>) -> El {
    text(s)
        .at_loc(Location::caller())
        .wrap_text()
        .fill_width()
        .height(Size::Hug)
}

#[track_caller]
pub fn h1(s: impl Into<String>) -> El {
    El::new(Kind::Heading)
        .at_loc(Location::caller())
        .style_profile(StyleProfile::TextOnly)
        .text(s)
        .text_role(TextRole::Display)
        .hug()
}

#[track_caller]
pub fn h2(s: impl Into<String>) -> El {
    El::new(Kind::Heading)
        .at_loc(Location::caller())
        .style_profile(StyleProfile::TextOnly)
        .text(s)
        .text_role(TextRole::Heading)
        .hug()
}

#[track_caller]
pub fn h3(s: impl Into<String>) -> El {
    El::new(Kind::Heading)
        .at_loc(Location::caller())
        .style_profile(StyleProfile::TextOnly)
        .text(s)
        .text_role(TextRole::Title)
        .hug()
}

#[track_caller]
pub fn mono(s: impl Into<String>) -> El {
    text(s).code()
}
