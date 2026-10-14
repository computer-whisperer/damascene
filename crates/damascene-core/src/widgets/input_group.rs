//! Input-group anatomy — one field trough shared by an input and its
//! leading/trailing companions.
//!
//! Oracle: shadcn/ui `InputGroup` / `InputGroupAddon` / `InputGroupText`
//! (`input-group.tsx`), per `docs/NAMING_ORACLE.md`. The group *is* the
//! field: it carries the Input-role chrome (trough fill, border, control
//! height, radius) and the inputs placed inside it render bare, so a
//! search icon, a unit suffix, a shortcut hint, or a small button all
//! sit inside the same single border instead of bolting boxes onto a
//! box.
//!
//! Children render in the order given — leading vs trailing is purely
//! positional, like DOM order in the oracle:
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! // Search field: leading icon, trailing shortcut hint.
//! input_group([
//!     input_group_addon(icon("search")),
//!     text_input_with("search", &self.query, &self.selection,
//!         TextInputOpts::default().placeholder("Search…")),
//!     input_group_addon(text("Ctrl K")),
//! ]);
//!
//! // Unit-suffixed numeric field.
//! input_group([
//!     text_input_with("width", &self.width_mm, &self.selection,
//!         TextInputOpts::default().tabular_numerals()),
//!     input_group_text("mm"),
//! ]);
//! ```
//!
//! The inner [`text_input`](crate::widgets::text_input) /
//! [`text_area`](crate::widgets::text_area) /
//! [`numeric_input`](crate::widgets::numeric_input) keeps its key, its
//! event contract (`apply_event` in the app's `on_event`, unchanged),
//! its caret, and its focusability — the group only strips the child's
//! own trough so the paint reads as one field.
//!
//! # Focus ring
//!
//! Focusing the inner input rings the **group**: the group sets
//! [`El::focus_within`] (CSS `:focus-within` per
//! `docs/NAMING_ORACLE.md`; the shadcn oracle's `has-[:focus-visible]`
//! treatment), so the stock ring wraps the whole trough while the
//! focused control's own ring is suppressed — one ring around one
//! field, never a ring cutting through the middle. Click focus counts
//! too (the group opts in via `always_show_focus_ring`, matching the
//! inner input's "now editable" affordance), the ring alpha follows
//! the same eased envelope as a focused node's, and the caret /
//! placeholder focus behaviors on the inner input are untouched.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::text;

/// What the de-chroming walk found inside one direct child of
/// [`input_group`].
#[derive(Clone, Copy, Default)]
struct Dechromed {
    /// The subtree contains (or is) a bare-able input control.
    control: bool,
    /// The subtree contains a `text_area` (the group then hugs its
    /// height instead of pinning the single-line control height —
    /// the oracle's `has-[>textarea]:h-auto`).
    text_area: bool,
}

/// Strip the field chrome from any `text_input` / `text_area` in the
/// subtree so it renders bare inside the group's trough.
///
/// Mechanism: set the control's `surface_role` to `None`. Post-b0699dd
/// the input recipes author no fill/stroke of their own — the trough
/// is entirely the Input role's *defaults* (`theme::apply_role_material`)
/// — so removing the role removes the paint; with no fill, no stroke,
/// and no role the paint pass emits no surface quad at all. Everything
/// else on the control is deliberately kept: key, focusability,
/// `capture_keys`, caret, padding (the event-time pointer→byte math
/// assumes it), and the focus ring (see the module doc's v1 note).
/// Authored paint (`.fill()` / `.stroke()` chained by the app) also
/// survives — the walk only clears the role.
///
/// The walk recurses through wrappers so a `numeric_input` (a row
/// around `{key}:field`) de-chromes its inner field, but does not
/// descend into a matched control's own children.
fn dechrome(el: &mut El) -> Dechromed {
    match el.kind {
        Kind::Custom("text_input") => {
            el.surface_role = SurfaceRole::None;
            Dechromed {
                control: true,
                text_area: false,
            }
        }
        Kind::Custom("text_area") => {
            el.surface_role = SurfaceRole::None;
            Dechromed {
                control: true,
                text_area: true,
            }
        }
        _ => {
            let mut found = Dechromed::default();
            for child in &mut el.children {
                let f = dechrome(child);
                found.control |= f.control;
                found.text_area |= f.text_area;
            }
            found
        }
    }
}

/// True for the cell kinds the group positions (addon / text cells).
fn is_cell(kind: &Kind) -> bool {
    matches!(
        kind,
        Kind::Custom("input_group_addon") | Kind::Custom("input_group_text")
    )
}

/// A row that IS the field (shadcn's `InputGroup`): Input-role trough,
/// control height, control radius — with its `text_input` /
/// `text_area` / `numeric_input` children rendered bare inside it (see
/// [`dechrome`]) and [`input_group_addon`] / [`input_group_text`]
/// cells for the leading/trailing furniture.
///
/// Child order is visual order; leading vs trailing is positional.
/// Cells before the first input get their padding on the outer (left)
/// side, cells after it on the right — the inner input's own
/// horizontal padding provides the cell↔text spacing, mirroring the
/// oracle's `pl-3` / `pr-3` addon alignment. A cell with authored
/// `.padding(...)` is left alone.
///
/// The group itself is not focusable — focus, caret, and key events
/// stay on the inner input (whose `apply_event` contract is
/// unchanged). With a `text_area` child the group hugs its height
/// instead of pinning the control rung.
#[track_caller]
pub fn input_group<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut children: Vec<El> = children.into_iter().map(Into::into).collect();

    let mut has_text_area = false;
    let mut control_flags = Vec::with_capacity(children.len());
    for child in &mut children {
        let found = dechrome(child);
        has_text_area |= found.text_area;
        control_flags.push(found.control);
    }

    // Positional cell padding: outer-side inset only. Between a cell
    // and the input the input's own SPACE_3 padding is the spacing;
    // between two stacked leading cells the second cell's left inset
    // separates them (symmetric on the trailing side).
    let first_control = control_flags.iter().position(|&is_control| is_control);
    let last_control = control_flags.iter().rposition(|&is_control| is_control);
    for (i, child) in children.iter_mut().enumerate() {
        if is_cell(&child.kind) && !child.explicit_padding {
            let leading = first_control.is_none_or(|fc| i < fc);
            let trailing = last_control.is_some_and(|lc| i > lc);
            child.padding = Sides {
                left: if leading { tokens::SPACE_3 } else { 0.0 },
                right: if trailing || first_control.is_none() {
                    tokens::SPACE_3
                } else {
                    0.0
                },
                top: 0.0,
                bottom: 0.0,
            };
        }
    }

    let mut group = El::new(Kind::Custom("input_group"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::Input)
        .surface_role(SurfaceRole::Input)
        // Group-level focus ring (module doc "Focus ring"): focusing
        // any inner control rings the trough, on click focus too, with
        // the ring band reserved outside the layout rect exactly as on
        // text_input.
        .focus_within()
        .always_show_focus_ring()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        // Trough fill + stroke come from the Input surface role as
        // *defaults* (theme::apply_role_material) — same contract as
        // text_input: authored .fill()/.stroke() wins.
        .default_radius(tokens::RADIUS_MD)
        .axis(Axis::Row)
        .align(Align::Center)
        .justify(Justify::Start)
        // Explicit zero padding and gap: edge insets come from the
        // cells and the inner input's own padding (which its caret
        // hit-testing math assumes), and being explicit stops the
        // metrics pass from stamping the Input rung's padding_x/gap
        // on top of them.
        .padding(Sides::all(0.0))
        .gap(0.0)
        .default_width(Size::Fill(1.0))
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT));
    if has_text_area {
        // Oracle: `has-[>textarea]:h-auto`. Explicit so the metrics
        // pass doesn't re-pin the control rung height.
        group = group.height(Size::Hug);
    }
    group.children(children)
}

/// Leading/trailing cell inside an [`input_group`] (shadcn's
/// `InputGroupAddon`): icons, kbd-style shortcut hints, small buttons.
/// Muted foreground, centered, tight padding, no grow.
///
/// Content whose color is unset (or left at the default `FOREGROUND` —
/// what `text(...)` and `icon(...)` produce) is tinted
/// `MUTED_FOREGROUND`; an authored non-default `.text_color(...)` (or
/// a button variant's own content color) survives.
#[track_caller]
pub fn input_group_addon(child: impl Into<El>) -> El {
    let mut child = child.into();
    stamp_muted(&mut child);
    El::new(Kind::Custom("input_group_addon"))
        .at_loc(Location::caller())
        .axis(Axis::Row)
        .align(Align::Center)
        .justify(Justify::Center)
        // Fallback for standalone use; inside input_group the
        // positional outer-side inset replaces this (see input_group).
        .default_padding(Sides::xy(tokens::SPACE_2, 0.0))
        .width(Size::Hug)
        .height(Size::Hug)
        .child(child)
}

/// Muted label cell for unit suffixes and prefixes inside an
/// [`input_group`] (shadcn's `InputGroupText`) — `"mm"`, `"px"`,
/// `"https://"`. Same cell anatomy as [`input_group_addon`] with a
/// `text(...)` leaf inside.
#[track_caller]
pub fn input_group_text(s: impl Into<String>) -> El {
    El::new(Kind::Custom("input_group_text"))
        .at_loc(Location::caller())
        .axis(Axis::Row)
        .align(Align::Center)
        .justify(Justify::Center)
        .default_padding(Sides::xy(tokens::SPACE_2, 0.0))
        .width(Size::Hug)
        .height(Size::Hug)
        .child(text(s).muted())
}

/// Tint text/icon content muted, without clobbering an authored color.
/// `text(...)` and `icon(...)` both stamp the default `FOREGROUND`
/// eagerly, so "unset or default-foreground" is the authored-vs-default
/// test; anything else (a button variant's content color, an explicit
/// `.text_color(...)`) is kept.
fn stamp_muted(el: &mut El) {
    if (el.text.is_some() || el.icon.is_some())
        && (el.text_color.is_none() || el.text_color == Some(tokens::FOREGROUND))
    {
        el.text_color = Some(tokens::MUTED_FOREGROUND);
    }
    for child in &mut el.children {
        stamp_muted(child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::icon;
    use crate::selection::Selection;
    use crate::widgets::numeric_input::{NumericInputOpts, numeric_input};
    use crate::widgets::text_area::text_area;
    use crate::widgets::text_input::{TextInputOpts, text_input, text_input_with};

    fn find<'a>(el: &'a El, kind: &'static str) -> Option<&'a El> {
        if matches!(el.kind, Kind::Custom(k) if k == kind) {
            return Some(el);
        }
        el.children.iter().find_map(|c| find(c, kind))
    }

    #[test]
    fn group_carries_input_surface_role_and_control_chrome() {
        let sel = Selection::default();
        let g = input_group([text_input("q", "", &sel)]);
        assert!(matches!(g.kind, Kind::Custom("input_group")));
        assert_eq!(g.surface_role, SurfaceRole::Input);
        assert!(matches!(g.style_profile, StyleProfile::Surface));
        assert_eq!(g.metrics_role, Some(MetricsRole::Input));
        assert_eq!(g.height, Size::Fixed(tokens::CONTROL_HEIGHT));
        // The group is chrome, not a control: focus stays inside.
        assert!(!g.focusable);
        assert!(!g.capture_keys);
    }

    #[test]
    fn group_claims_descendant_focus_ring() {
        // Module doc "Focus ring": the group carries the
        // `:focus-within` flag so focusing any inner control rings the
        // trough — on click focus too — with the ring band reserved
        // outside the layout rect exactly as on text_input.
        let sel = Selection::default();
        let g = input_group([text_input("q", "", &sel)]);
        assert!(g.focus_within);
        assert!(g.always_show_focus_ring);
        assert_eq!(g.paint_overflow, Sides::all(tokens::RING_WIDTH));
    }

    #[test]
    fn inner_text_input_is_dechromed_but_keeps_key_and_focus() {
        let sel = Selection::default();
        let g = input_group([text_input("q", "hello", &sel)]);
        let input = find(&g, "text_input").expect("inner text_input");
        // De-chromed: the Input role (the entire trough post-b0699dd)
        // is gone, and the recipe authors no fill/stroke of its own.
        assert_eq!(input.surface_role, SurfaceRole::None);
        assert!(input.fill.is_none());
        assert!(input.stroke.is_none());
        // Alive: key, focus, key capture, caret padding.
        assert_eq!(input.key.as_deref(), Some("q"));
        assert!(input.focusable);
        assert!(input.capture_keys);
    }

    #[test]
    fn dechrome_reaches_numeric_input_inner_field() {
        let sel = Selection::default();
        let g = input_group([numeric_input(
            "w",
            "42",
            &sel,
            NumericInputOpts::default(),
        )]);
        let field = find(&g, "text_input").expect("numeric_input inner field");
        assert_eq!(field.surface_role, SurfaceRole::None);
        assert_eq!(field.key.as_deref(), Some("w:field"));
        assert!(field.focusable);
    }

    #[test]
    fn text_area_child_is_dechromed_and_group_hugs_height() {
        let sel = Selection::default();
        let g = input_group([text_area("notes", "line", &sel)]);
        let area = find(&g, "text_area").expect("inner text_area");
        assert_eq!(area.surface_role, SurfaceRole::None);
        assert_eq!(g.height, Size::Hug);
    }

    #[test]
    fn addon_is_muted_centered_and_unfilled() {
        let a = input_group_addon(icon("search"));
        assert!(matches!(a.kind, Kind::Custom("input_group_addon")));
        assert!(a.fill.is_none());
        assert!(a.stroke.is_none());
        assert_eq!(a.width, Size::Hug);
        assert!(matches!(a.align, Align::Center));
        // icon() stamps default FOREGROUND; the addon re-tints it muted.
        assert_eq!(a.children[0].text_color, Some(tokens::MUTED_FOREGROUND));
    }

    #[test]
    fn addon_keeps_authored_content_color() {
        let a = input_group_addon(icon("search").text_color(tokens::PRIMARY));
        assert_eq!(a.children[0].text_color, Some(tokens::PRIMARY));
    }

    #[test]
    fn group_text_is_a_muted_label_cell() {
        let t = input_group_text("mm");
        assert!(matches!(t.kind, Kind::Custom("input_group_text")));
        assert!(t.fill.is_none());
        let label = &t.children[0];
        assert!(matches!(label.kind, Kind::Text));
        assert_eq!(label.text.as_deref(), Some("mm"));
        assert_eq!(label.text_color, Some(tokens::MUTED_FOREGROUND));
    }

    #[test]
    fn child_order_is_visual_order() {
        let sel = Selection::default();
        let g = input_group([
            input_group_addon(icon("search")),
            text_input_with(
                "q",
                "",
                &sel,
                TextInputOpts::default().placeholder("Search…"),
            ),
            input_group_text("Ctrl K"),
        ]);
        let kinds: Vec<_> = g.children.iter().map(|c| &c.kind).collect();
        assert!(matches!(kinds[0], Kind::Custom("input_group_addon")));
        assert!(matches!(kinds[1], Kind::Custom("text_input")));
        assert!(matches!(kinds[2], Kind::Custom("input_group_text")));
    }

    #[test]
    fn cells_get_outer_side_padding_by_position() {
        let sel = Selection::default();
        let g = input_group([
            input_group_addon(icon("search")),
            text_input("q", "", &sel),
            input_group_text("mm"),
        ]);
        let leading = &g.children[0];
        assert_eq!(leading.padding.left, tokens::SPACE_3);
        assert_eq!(leading.padding.right, 0.0);
        let trailing = &g.children[2];
        assert_eq!(trailing.padding.left, 0.0);
        assert_eq!(trailing.padding.right, tokens::SPACE_3);
        // The group itself stays zero-padded: insets live on the cells
        // and the input's own padding (caret math depends on it).
        assert_eq!(g.padding, Sides::all(0.0));
        assert!(g.explicit_padding);
    }

    #[test]
    fn authored_cell_padding_survives_positioning() {
        let sel = Selection::default();
        let g = input_group([
            input_group_addon(icon("search")).padding(Sides::all(2.0)),
            text_input("q", "", &sel),
        ]);
        assert_eq!(g.children[0].padding, Sides::all(2.0));
    }
}
