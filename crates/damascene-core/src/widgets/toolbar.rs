//! Toolbar anatomy — compact action rows for page and table controls.
//!
//! # Buttons inside a toolbar are ghost
//!
//! A toolbar is where a tool keeps its verbs, and every desktop tool
//! draws those verbs borderless: GitKraken's Pull / Push / Branch,
//! VS Code's editor actions, a Figma toolbar. A row of boxed
//! secondary buttons there reads as a form, and once badges and
//! segments are boxed too nothing on the screen says what is
//! clickable (Opus review §6.7; ruled 2026-10-14,
//! `docs/POLISH_REVIEW_2026_10.md`). So [`toolbar`] and
//! [`toolbar_group`] restyle the controls handed to them: a button
//! still wearing its recipe default becomes `.ghost()`. A button whose
//! author chose a variant — `.primary()` for the one solid action,
//! `.secondary()` to keep the box, `.outline()`, `.destructive()` — is
//! left exactly as asked. The rule applies through a [`button_group`]
//! or [`split_button`] inside the toolbar, whose seams are re-resolved
//! afterwards so the ghost halves still divide.
//!
//! [`button_group`]: crate::widgets::button_group::button_group
//! [`split_button`]: crate::widgets::button_group::split_button

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::a11y::Role;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::button_group;
use crate::widgets::text::{h3, text};

/// A compact action row for page and table controls.
///
/// Brings no padding and no surface of its own — it's a transparent
/// `row` that expects a padded container around it. At the window root
/// that container is [`page`](crate::widgets::page::page) (which bakes
/// the window padding); inside a card it's the `card_header` /
/// `card_content` slots. A `toolbar` placed directly in an unpadded
/// root sits flush against the window edge and clips under rounded
/// window corners.
///
/// Buttons in their default look are made `.ghost()`; see the module
/// docs for the rule and how to keep a box.
#[track_caller]
pub fn toolbar<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut items: Vec<El> = children.into_iter().map(Into::into).collect();
    ghost_default_actions(&mut items);
    row(items)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .gap(tokens::SPACE_2)
        .align(Align::Center)
}

/// Hug-sized cluster of related actions inside a [`toolbar`] — keeps its
/// buttons together so `spacer()` can push groups apart. Applies the
/// same ghost-by-default rule as the toolbar.
#[track_caller]
pub fn toolbar_group<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut items: Vec<El> = children.into_iter().map(Into::into).collect();
    ghost_default_actions(&mut items);
    let mut group = row(items)
        .at_loc(Location::caller())
        .width(Size::Hug)
        .height(Size::Hug)
        .gap(tokens::SPACE_2)
        .align(Align::Center);
    group.kind = Kind::Custom("toolbar_group");
    group
}

/// The toolbar rule, in place: a default-look solid button goes ghost;
/// a composite (a `button_group`, a `split_button`) or a nested
/// `toolbar_group` is entered, and a joined group is re-joined so its
/// seams follow the new strokes.
fn ghost_default_actions(items: &mut [El]) {
    for item in items.iter_mut() {
        if item.style_profile == StyleProfile::Composite
            || item.kind == Kind::Custom("toolbar_group")
        {
            ghost_default_actions(&mut item.children);
            button_group::rejoin(item);
        } else if is_default_solid_button(item) {
            *item = std::mem::take(item).ghost();
        }
    }
}

fn is_default_solid_button(el: &El) -> bool {
    el.style_profile == StyleProfile::Solid
        && !el.explicit_variant
        && el.a11y.as_deref().and_then(|a| a.role) == Some(Role::Button)
}

/// Toolbar heading — an [`h3`] with line height tightened to match the
/// toolbar's compact row.
#[track_caller]
pub fn toolbar_title(title: impl Into<String>) -> El {
    h3(title)
        .at_loc(Location::caller())
        .default_line_height(tokens::TEXT_BASE.size)
}

/// Muted single-line summary text next to the title — fills the remaining
/// row width and truncates with an ellipsis.
#[track_caller]
pub fn toolbar_description(description: impl Into<String>) -> El {
    text(description)
        .at_loc(Location::caller())
        .muted()
        .ellipsis()
        .width(Size::Fill(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::button::{button, button_with_icon, icon_button, icon_button_with_caption};
    use crate::widgets::button_group::split_button;
    use crate::widgets::toggle::toggle;

    #[test]
    fn toolbar_centers_action_rows() {
        let bar = toolbar([toolbar_title("Documents"), spacer(), button("Upload")]);

        assert_eq!(bar.axis, Axis::Row);
        assert_eq!(bar.align, Align::Center);
        assert_eq!(bar.width, Size::Fill(1.0));
        assert_eq!(bar.gap, tokens::SPACE_2);
    }

    #[test]
    fn toolbar_group_hugs_inline_actions() {
        let group = toolbar_group([button("Filter"), button("Export")]);

        assert_eq!(group.width, Size::Hug);
        assert_eq!(group.align, Align::Center);
        assert_eq!(group.kind, Kind::Custom("toolbar_group"));
    }

    #[test]
    fn default_buttons_in_a_toolbar_go_ghost_and_chosen_variants_stay() {
        let bar = toolbar([
            button("Fetch"),
            button_with_icon("download", "Pull"),
            icon_button("settings").aria_label("Settings"),
            icon_button_with_caption("upload", "Push"),
            button("Keep").secondary(),
            button("Run").primary(),
            button("Discard").destructive(),
            toggle("wrap", true, "Wrap"),
            text("status"),
        ]);
        let [
            fetch,
            pull,
            settings,
            push,
            keep,
            run,
            discard,
            wrap,
            status,
        ] = bar.children.iter().collect::<Vec<_>>()[..]
        else {
            panic!("nine children");
        };
        for ghosted in [fetch, pull, settings, push] {
            assert!(
                ghosted.fill.is_none(),
                "default button goes ghost: {:?}",
                ghosted.kind
            );
            assert!(ghosted.stroke.is_none());
            assert_eq!(ghosted.text_color, Some(tokens::MUTED_FOREGROUND));
        }
        assert_eq!(keep.fill, Some(tokens::SECONDARY), "asked for the box");
        assert_eq!(run.fill, Some(tokens::PRIMARY), "the one solid action");
        assert_eq!(discard.fill, Some(tokens::DESTRUCTIVE));
        assert_eq!(
            wrap.fill,
            Some(tokens::ACCENT),
            "a toggle is not a button variant and keeps its pressed surface"
        );
        assert!(status.fill.is_none() && status.text.is_some());
    }

    #[test]
    fn the_rule_reaches_through_groups_and_keeps_the_split_seam() {
        let bar = toolbar([
            toolbar_group([button("Undo"), button("Redo")]),
            split_button("pull", "Pull"),
            split_button("push", "Push").primary(),
        ]);
        let group = &bar.children[0];
        for b in &group.children {
            assert!(b.fill.is_none(), "nested toolbar_group buttons go ghost");
        }

        let pull = &bar.children[1];
        assert_eq!(pull.kind, Kind::Custom("split_button"));
        for half in &pull.children {
            assert!(half.fill.is_none(), "both halves ghost");
        }
        // Two ghost halves have no stroke to mark the boundary, so the
        // re-join installs the seam on the chevron's left edge.
        let chevron = &pull.children[1];
        let seam = chevron.border.as_deref().expect("seam border");
        assert_eq!(seam.widths.left, 1.0);

        let push = &bar.children[2];
        for half in &push.children {
            assert_eq!(
                half.fill,
                Some(tokens::PRIMARY),
                "a chosen variant survives"
            );
        }
    }
}
