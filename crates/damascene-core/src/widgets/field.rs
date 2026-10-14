//! Field — shadcn-shaped labelled control for settings rows and
//! inspector panes.
//!
//! Oracle: shadcn/ui's `Field` family (`Field`, `FieldContent`,
//! `FieldDescription`, `FieldGroup`, `FieldSeparator`, `FieldSet`,
//! `FieldLegend` — see
//! `references/workbench-validation/shadcn-refs/src/components/ui/field.tsx`),
//! per `docs/NAMING_ORACLE.md`. This is the preferred path for the two
//! shapes the validation rounds kept missing: the **settings row with a
//! description** and the **inspector row**.
//!
//! - [`field`] — label above control (shadcn's default
//!   `orientation="vertical"`).
//! - [`field_with`] + [`FieldOpts`] — adds `description(...)` plus the
//!   two side-by-side shapes:
//!   - `horizontal()` — the **settings row**: label + description
//!     stacked on the left, control trailing right, vertically
//!     centered. Pair with `control_width(...)` to land a page of
//!     trailing selects / inputs on one right edge.
//!   - `inline_label(w)` — the **inspector row**: fixed label gutter
//!     on the left, control filling the remainder, so a stack of rows
//!     reads as a table.
//! - [`field_group`] / [`field_separator`] / [`field_set`] — the
//!   surrounding rhythm: stacks, rules, and legends.
//!
//! The canonical settings row:
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct Prefs { smooth_scrolling: bool }
//!
//! impl App for Prefs {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         field_group([field_with(
//!             "Smooth scrolling",
//!             checkbox("smooth_scrolling", self.smooth_scrolling),
//!             FieldOpts::default()
//!                 .description("Animate scroll position instead of jumping by line.")
//!                 .horizontal(),
//!         )])
//!     }
//! }
//! ```
//!
//! And the canonical inspector pane — one gutter for the whole
//! column, controls flush on a common left edge:
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! const GUTTER: f32 = 72.0;
//!
//! fn prop(label: &str, control: impl Into<El>) -> El {
//!     field_with(label, control, FieldOpts::default().inline_label(GUTTER))
//! }
//! ```
//!
//! For a full vertical form with validation messages
//! ([`crate::widgets::form::form_message`]) or an explicit control
//! wrapper ([`crate::widgets::form::form_control`]), the
//! [`crate::widgets::form`] anatomy still serves;
//! [`crate::widgets::form::field_row`] remains the bare
//! label-spacer-control primitive for one-off rows that want neither
//! shape here.
//!
//! # Dogfood note
//!
//! Pure composition over the public widget-kit surface — `row`,
//! `column`, [`crate::widgets::text::text`] role modifiers, and the
//! stock [`crate::widgets::separator::separator`]. No internal
//! machinery; an app crate can fork this file and produce an
//! equivalent helper.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use super::separator::separator;
use super::text::text;
use crate::metrics::MetricsRole;
use crate::tokens;
use crate::tree::*;

/// Optional configuration for [`field_with`]. The defaults reproduce
/// [`field`] verbatim, so callers only set what they need — the same
/// shape as [`crate::widgets::text_input::TextInputOpts`].
#[derive(Clone, Copy, Debug, Default)]
pub struct FieldOpts<'a> {
    /// One dim helper line (shadcn's `FieldDescription` —
    /// `text-muted-foreground`). Vertical fields place it under the
    /// control; horizontal fields stack it under the label in the
    /// leading column, matching field.tsx's `FieldContent` composition.
    pub description: Option<&'a str>,
    /// Switch to shadcn's `orientation="horizontal"`: leading column of
    /// label + description, control trailing right, vertically
    /// centered. The settings-row shape.
    pub horizontal: bool,
    /// Pin the trailing control to a fixed width, in logical pixels.
    ///
    /// Only [`FieldOpts::horizontal`] rows read this — see the
    /// orientation table on [`field_with`]. Most controls that trail a
    /// settings row default to `Size::Fill(1.0)`
    /// ([`crate::widgets::select::select_trigger`],
    /// [`crate::widgets::text_input::text_input`]), which splits the
    /// row down the middle against the `Fill` label column. Giving a
    /// page of fields the same `control_width` lands every control on
    /// one right-aligned edge without chaining `.width(...)` on each
    /// one.
    ///
    /// Stamped as a *default* width, so two kinds of control keep
    /// their own size instead (see "Control sizing" on [`field_with`]):
    /// one the caller sized with an explicit `.width(...)`, and one
    /// whose width the metrics pass owns — a switch or a checkbox in a
    /// pinned row stays its own small self rather than stretching to
    /// `w`.
    pub control_width: Option<f32>,
    /// The inspector shape: a fixed-width label gutter on the left, in
    /// logical pixels, with the control taking the remainder of the
    /// row.
    ///
    /// This is the row a properties pane is built from — label column
    /// aligned down the whole pane, controls left-aligned on a common
    /// edge so the stack reads as a table. It is *not*
    /// [`FieldOpts::horizontal`], whose label column is `Fill` and
    /// whose control trails right, sized for a switch or a button.
    ///
    /// The label ellipsizes at the gutter; the control's left edge
    /// lands at `width + `[`tokens::SPACE_3`]. The control is grown to
    /// fill the rest of the row under the rules in "Control sizing" on
    /// [`field_with`] — so selects, inputs, and numeric inputs reach
    /// the pane's right edge while a switch or checkbox stays its own
    /// size at the gutter edge, which is what an inspector wants of
    /// each.
    ///
    /// A [`FieldOpts::description`] is placed full width *under* the
    /// row rather than inside the gutter — an inspector description
    /// that started at the control edge would read as part of the
    /// control.
    ///
    /// Takes precedence over [`FieldOpts::horizontal`]; see the
    /// orientation table on [`field_with`].
    pub inline_label: Option<f32>,
}

impl<'a> FieldOpts<'a> {
    /// Set the dim helper line (see [`FieldOpts::description`]).
    pub fn description(mut self, d: &'a str) -> Self {
        self.description = Some(d);
        self
    }

    /// Use the horizontal settings-row orientation (see
    /// [`FieldOpts::horizontal`]).
    pub fn horizontal(mut self) -> Self {
        self.horizontal = true;
        self
    }

    /// Pin the trailing control's width in a horizontal row (see
    /// [`FieldOpts::control_width`]).
    pub fn control_width(mut self, w: f32) -> Self {
        self.control_width = Some(w);
        self
    }

    /// Use the inspector shape — fixed label gutter, control fills the
    /// rest (see [`FieldOpts::inline_label`]).
    pub fn inline_label(mut self, width: f32) -> Self {
        self.inline_label = Some(width);
        self
    }
}

/// A labelled control (shadcn's `Field`, default vertical orientation):
/// label ([`TextRole::Label`]) above the control, both full width.
///
/// For a description line or the horizontal settings-row shape, use
/// [`field_with`]. For a compact label/control pair with no
/// description, [`crate::widgets::form::field_row`] also serves.
#[track_caller]
pub fn field(label: impl Into<String>, control: impl Into<El>) -> El {
    field_with(label, control, FieldOpts::default())
}

/// Like [`field`], but takes a [`FieldOpts`] for a description line
/// and/or a different orientation. Pass `FieldOpts::default()` for an
/// output identical to [`field`].
///
/// # Orientation
///
/// Three shapes, resolved in this order — the first one set wins, so
/// no combination panics or silently produces a hybrid:
///
/// | opts | shape | description goes |
/// |---|---|---|
/// | [`inline_label(w)`](FieldOpts::inline_label) | `[label: w px][control: fill]` — the inspector row | full width under the row |
/// | [`horizontal()`](FieldOpts::horizontal) | `[label + description: fill][control]` — the settings row | under the label, in the leading column |
/// | (default) | label above control, both full width | under the control |
///
/// `inline_label` beating `horizontal` is the deliberate choice: it is
/// the strictly more specified layout (it pins the label column *and*
/// the control's growth), so a caller who set both wanted the gutter.
/// [`control_width`](FieldOpts::control_width) is read only by the
/// horizontal shape — in the inline-label shape the control is defined
/// as taking the remainder of the gutter row, and in the vertical
/// shape there is no trailing control to pin.
///
/// # Control sizing
///
/// Both side-by-side shapes want a say in how wide the control is —
/// `inline_label` grows it to the row's remainder, `control_width`
/// pins it to a shared size. Neither overrides a control that already
/// knows its own width. In precedence order:
///
/// 1. An explicit `.width(...)` on the control wins outright.
/// 2. A control whose width the metrics pass owns (switch, checkbox,
///    radio — anything with a size-scaled [`MetricsRole`]) gets its
///    width re-stamped after this function returns, so it keeps its
///    intrinsic size. That is deliberate: a stretched switch is a
///    broken switch, and every real inspector leaves choice controls
///    small and left-aligned at the gutter.
/// 3. Everything else — the `Fill`-by-default select / input / text
///    area, and the fixed-by-default
///    [`crate::widgets::numeric_input::numeric_input`] — takes the
///    shape's width.
#[track_caller]
pub fn field_with(label: impl Into<String>, control: impl Into<El>, opts: FieldOpts<'_>) -> El {
    let label_el = text(label)
        .at_loc(Location::caller())
        .label()
        .ellipsis()
        .width(Size::Fill(1.0));

    if let Some(gutter) = opts.inline_label {
        // The inspector row: fixed label gutter, control takes the
        // rest. Not a shadcn shape — `Field` has no fixed-gutter
        // orientation — but the one every properties pane converges
        // on, and the shape apps kept hand-rolling next to this
        // anatomy.
        let label_el = label_el.width(Size::Fixed(gutter));
        let control = size_control(opts_control(control), Size::Fill(1.0));

        let row_el = row([label_el, control])
            .at_loc(Location::caller())
            .align(Align::Center)
            .default_gap(tokens::SPACE_3)
            .width(Size::Fill(1.0))
            .height(Size::Hug);

        let Some(d) = opts.description else {
            return row_el.metrics_role(MetricsRole::FormItem);
        };
        // Full width under the row, not indented into the control
        // column: an inspector's helper line reads as belonging to the
        // row, and starting it at the control edge would make it look
        // like part of the control. SPACE_1 keeps it bound to the row
        // the way the horizontal shape binds it to its label.
        return column([row_el, field_description(d)])
            .at_loc(Location::caller())
            .metrics_role(MetricsRole::FormItem)
            .default_gap(tokens::SPACE_1)
            .width(Size::Fill(1.0))
            .height(Size::Hug);
    }

    if opts.horizontal {
        // shadcn `orientation="horizontal"`: `FieldContent` (label +
        // description, `flex-1`) leads; the control trails right.
        // field.tsx's `gap-1.5` (6px) content gap translates to
        // SPACE_1, the nearest step on the damascene scale — it keeps
        // label + description reading as one unit.
        let mut content = vec![label_el];
        if let Some(d) = opts.description {
            content.push(field_description(d));
        }
        let content = column(content)
            .default_gap(tokens::SPACE_1)
            .width(Size::Fill(1.0))
            .height(Size::Hug);

        // `control_width` pins the trailing control so a page of rows
        // shares one right edge. `default_width` rather than `width`:
        // the stock controls that need pinning declare `Fill(1.0)` as
        // a default too, so this claims the slot without overriding a
        // caller who already sized the control themselves.
        let mut trailing = opts_control(control);
        if let Some(w) = opts.control_width {
            trailing = size_control(trailing, Size::Fixed(w));
        }

        row([content, trailing])
            .at_loc(Location::caller())
            .metrics_role(MetricsRole::FormItem)
            .align(Align::Center)
            .default_gap(tokens::SPACE_3)
            .width(Size::Fill(1.0))
            .height(Size::Hug)
    } else {
        // shadcn vertical `Field`: `flex-col gap-3`, children full
        // width; the description sits under the control.
        let mut children = vec![label_el, opts_control(control)];
        if let Some(d) = opts.description {
            children.push(field_description(d));
        }
        column(children)
            .at_loc(Location::caller())
            .metrics_role(MetricsRole::FormItem)
            .default_gap(tokens::SPACE_3)
            .width(Size::Fill(1.0))
            .height(Size::Hug)
    }
}

/// The control slot passes through untouched so apps reading routed
/// events via `target_key` see the control's own key.
fn opts_control(control: impl Into<El>) -> El {
    control.into()
}

/// Claim the control's width slot for a shape that needs it, without
/// overriding a caller who already sized the control.
///
/// `El::default_width` is unconditional — it's meant to run *inside* a
/// constructor, before the caller has had a chance to chain
/// `.width(...)`. Applied after the fact, as `field_with` does, it
/// needs the `explicit_width` guard the layout pass would otherwise
/// have honoured.
fn size_control(control: El, w: Size) -> El {
    if control.explicit_width {
        control
    } else {
        control.default_width(w)
    }
}

/// The dim helper line (shadcn's `FieldDescription` — `text-sm
/// text-muted-foreground`, rendered here in the caption role so it
/// steps down from the label the way the settings references do).
#[track_caller]
fn field_description(description: impl Into<String>) -> El {
    text(description)
        .at_loc(Location::caller())
        .caption()
        .muted()
        .wrap_text()
        .fill_width()
}

/// Vertical stack of [`field`]s (shadcn's `FieldGroup`, `flex-col
/// gap-7` → [`tokens::SPACE_7`] on the damascene scale).
///
/// Carries `RING_WIDTH` of horizontal default padding so trailing
/// controls flush at the group's edges keep room for their focus-ring
/// band inside a clipping ancestor (`scroll(...)`) — the same reserve
/// [`crate::widgets::form::form`] and
/// [`crate::widgets::item::item_group`] make.
#[track_caller]
pub fn field_group<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .metrics_role(MetricsRole::Form)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_gap(tokens::SPACE_7)
        .default_padding(Sides::xy(tokens::RING_WIDTH, 0.0))
}

/// Hairline rule between fields (shadcn's `FieldSeparator`) — the
/// stock [`crate::widgets::separator::separator`] under the anatomy's
/// name.
#[track_caller]
pub fn field_separator() -> El {
    separator().at_loc(Location::caller())
}

/// A legended group of fields (shadcn's `FieldSet` + `FieldLegend`):
/// a small medium-weight legend (field.tsx's legend tone — `text-base
/// font-medium`, `mb-3` → [`tokens::SPACE_3`]) above a stack of fields
/// at the fieldset rhythm (`gap-6` → [`tokens::SPACE_6`]).
#[track_caller]
pub fn field_set<I, E>(legend: impl Into<String>, children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let legend_el = text(legend)
        .at_loc(Location::caller())
        .title()
        .font_weight(FontWeight::Medium)
        .ellipsis()
        .width(Size::Fill(1.0));

    let group = column(children)
        .at_loc(Location::caller())
        .default_gap(tokens::SPACE_6)
        .width(Size::Fill(1.0))
        .height(Size::Hug);

    column([legend_el, group])
        .at_loc(Location::caller())
        .metrics_role(MetricsRole::Form)
        .default_gap(tokens::SPACE_3)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_padding(Sides::xy(tokens::RING_WIDTH, 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::Selection;
    use crate::widgets::checkbox::checkbox;
    use crate::widgets::select::select_trigger;
    use crate::widgets::switch::switch;
    use crate::widgets::text_input::text_input;

    #[test]
    fn field_stacks_label_above_control() {
        // shadcn's default vertical orientation: label first, control
        // second, in a full-width column at the Field gap.
        let f = field("Email", switch("notify", true));

        assert_eq!(f.axis, Axis::Column);
        assert_eq!(f.metrics_role, Some(MetricsRole::FormItem));
        assert_eq!(f.width, Size::Fill(1.0));
        assert_eq!(f.gap, tokens::SPACE_3);
        assert_eq!(f.children.len(), 2);

        let label = &f.children[0];
        assert_eq!(label.text.as_deref(), Some("Email"));
        assert_eq!(label.text_role, TextRole::Label);

        let control = &f.children[1];
        assert_eq!(control.key.as_deref(), Some("notify"));
        assert!(f.key.is_none(), "field carries no key of its own");
    }

    #[test]
    fn vertical_description_sits_under_the_control() {
        let f = field_with(
            "Theme",
            switch("theme", false),
            FieldOpts::default().description("Applies to all windows."),
        );

        assert_eq!(f.children.len(), 3);
        let description = &f.children[2];
        assert_eq!(description.text.as_deref(), Some("Applies to all windows."));
        assert_eq!(description.text_role, TextRole::Caption);
        assert_eq!(description.text_color, Some(tokens::MUTED_FOREGROUND));
    }

    #[test]
    fn horizontal_stacks_description_under_label_in_leading_column() {
        // The settings-row shape: FieldContent (label + description)
        // leads; the control trails; the row is vertically centered.
        let f = field_with(
            "Smooth scrolling",
            checkbox("smooth", true),
            FieldOpts::default()
                .description("Animate scroll position instead of jumping by line.")
                .horizontal(),
        );

        assert_eq!(f.axis, Axis::Row);
        assert_eq!(f.align, Align::Center);
        assert_eq!(f.width, Size::Fill(1.0));
        assert_eq!(f.children.len(), 2);

        let content = &f.children[0];
        assert_eq!(content.axis, Axis::Column);
        assert_eq!(content.width, Size::Fill(1.0));
        assert_eq!(content.gap, tokens::SPACE_1);
        assert_eq!(content.children.len(), 2);
        assert_eq!(content.children[0].text.as_deref(), Some("Smooth scrolling"));
        assert_eq!(content.children[0].text_role, TextRole::Label);
        assert_eq!(content.children[1].text_role, TextRole::Caption);
        assert_eq!(
            content.children[1].text_color,
            Some(tokens::MUTED_FOREGROUND)
        );

        let control = &f.children[1];
        assert_eq!(control.key.as_deref(), Some("smooth"));
    }

    #[test]
    fn horizontal_control_trails_right_and_centers_vertically() {
        // Layout-level check of the settings-row promise: the leading
        // column absorbs the slack, so the control's right edge lands
        // on the row's right edge, vertically centered against the
        // two-line label block.
        let mut root = field_with(
            "Reduce motion",
            switch("reduce_motion", false),
            FieldOpts::default()
                .description("Disable panel transitions, easing, and decorative animation.")
                .horizontal(),
        );
        let mut state = crate::UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 600.0, 200.0));

        let row_rect = root.computed_rect;
        let control = &root.children[1];
        let control_right = control.computed_rect.x + control.computed_rect.w;
        assert!(
            (control_right - (row_rect.x + row_rect.w)).abs() < 0.5,
            "control should land flush right, right edge {} vs row {}",
            control_right,
            row_rect.x + row_rect.w
        );

        let row_mid = row_rect.y + row_rect.h / 2.0;
        let control_mid = control.computed_rect.y + control.computed_rect.h / 2.0;
        assert!(
            (control_mid - row_mid).abs() < 0.5,
            "control should center vertically, mid {} vs row mid {}",
            control_mid,
            row_mid
        );

        let content = &root.children[0];
        assert!(
            content.computed_rect.w > control.computed_rect.w,
            "leading column should absorb the row's slack"
        );
    }

    #[test]
    fn control_width_pins_the_trailing_control_in_horizontal() {
        let f = field_with(
            "Theme",
            select_trigger("theme", "System"),
            FieldOpts::default().horizontal().control_width(140.0),
        );
        let control = &f.children[1];
        assert_eq!(
            control.width,
            Size::Fixed(140.0),
            "select_trigger's Fill(1.0) default should give way to the pin"
        );
        assert!(
            !control.explicit_width,
            "stamped as a default so an authored width still wins"
        );
    }

    #[test]
    fn control_width_is_only_read_by_the_horizontal_shape() {
        // Vertical: the control is full width by construction.
        let vertical = field_with(
            "Theme",
            select_trigger("theme", "System"),
            FieldOpts::default().control_width(140.0),
        );
        assert_eq!(vertical.children[1].width, Size::Fill(1.0));

        // Inline-label: the control is defined as filling the
        // remainder of the gutter row.
        let inline = field_with(
            "Theme",
            select_trigger("theme", "System"),
            FieldOpts::default().inline_label(72.0).control_width(140.0),
        );
        assert_eq!(inline.children[1].width, Size::Fill(1.0));
    }

    #[test]
    fn control_width_yields_to_an_authored_control_width() {
        let f = field_with(
            "Theme",
            select_trigger("theme", "System").width(Size::Fixed(60.0)),
            FieldOpts::default().horizontal().control_width(140.0),
        );
        assert_eq!(f.children[1].width, Size::Fixed(60.0));
    }

    #[test]
    fn control_width_lands_a_page_of_rows_on_one_edge() {
        // The promise: two rows with different label lengths and
        // different trailing controls share a control left edge and a
        // control right edge.
        const W: f32 = 140.0;
        let opts = FieldOpts::default().horizontal().control_width(W);
        let mut root = field_group([
            field_with("Theme", select_trigger("theme", "System"), opts),
            field_with(
                "Default editor font family",
                select_trigger("font", "Inter"),
                opts,
            ),
        ]);
        let mut state = crate::UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 480.0, 200.0));

        let a = state.rect_of_key("theme").expect("first control");
        let b = state.rect_of_key("font").expect("second control");
        assert!((a.w - W).abs() < 0.5, "first control width {}", a.w);
        assert!((b.w - W).abs() < 0.5, "second control width {}", b.w);
        assert!(
            (a.x - b.x).abs() < 0.5,
            "controls should share one left edge, {} vs {}",
            a.x,
            b.x
        );
    }

    #[test]
    fn inline_label_pins_the_gutter_and_fills_the_control() {
        let f = field_with(
            "Footprint",
            select_trigger("footprint", "0603"),
            FieldOpts::default().inline_label(72.0),
        );

        assert_eq!(f.axis, Axis::Row);
        assert_eq!(f.align, Align::Center);
        assert_eq!(f.metrics_role, Some(MetricsRole::FormItem));
        assert_eq!(f.width, Size::Fill(1.0));
        assert_eq!(f.gap, tokens::SPACE_3);
        assert_eq!(f.children.len(), 2);

        let label = &f.children[0];
        assert_eq!(label.text.as_deref(), Some("Footprint"));
        assert_eq!(label.text_role, TextRole::Label);
        assert_eq!(label.width, Size::Fixed(72.0));
        assert_eq!(
            label.text_overflow,
            TextOverflow::Ellipsis,
            "a gutter narrower than the label must ellipsize, not wrap"
        );

        let control = &f.children[1];
        assert_eq!(control.key.as_deref(), Some("footprint"));
        assert_eq!(control.width, Size::Fill(1.0));
    }

    #[test]
    fn inline_label_takes_precedence_over_horizontal() {
        // Both set: the more specified shape wins, and nothing panics.
        let f = field_with(
            "Footprint",
            select_trigger("footprint", "0603"),
            FieldOpts::default().inline_label(72.0).horizontal(),
        );
        assert_eq!(f.axis, Axis::Row);
        assert_eq!(f.children[0].width, Size::Fixed(72.0));
        assert_eq!(
            f.children[0].text.as_deref(),
            Some("Footprint"),
            "leading child is the label itself, not a FieldContent column"
        );
    }

    #[test]
    fn inline_label_description_sits_full_width_under_the_row() {
        let f = field_with(
            "Footprint",
            select_trigger("footprint", "0603"),
            FieldOpts::default()
                .inline_label(72.0)
                .description("From the active library."),
        );

        assert_eq!(f.axis, Axis::Column);
        assert_eq!(f.metrics_role, Some(MetricsRole::FormItem));
        assert_eq!(f.gap, tokens::SPACE_1);
        assert_eq!(f.children.len(), 2);

        let row_el = &f.children[0];
        assert_eq!(row_el.axis, Axis::Row);
        assert_eq!(row_el.children[0].width, Size::Fixed(72.0));

        let description = &f.children[1];
        assert_eq!(
            description.text.as_deref(),
            Some("From the active library.")
        );
        assert_eq!(description.text_role, TextRole::Caption);
        assert_eq!(description.text_color, Some(tokens::MUTED_FOREGROUND));
        assert_eq!(
            description.width,
            Size::Fill(1.0),
            "full width under the row, not indented into the control column"
        );
    }

    #[test]
    fn choice_controls_keep_their_intrinsic_width_in_both_shapes() {
        // Both shapes claim the control's width slot, but the metrics
        // pass owns switch / checkbox sizing and re-stamps it — so an
        // inspector row and a pinned settings row both leave them
        // small, which is what every real properties pane does.
        let theme = crate::theme::Theme::default();
        let mut inline = field_with(
            "Supports",
            switch("supports", true),
            FieldOpts::default().inline_label(72.0),
        );
        let mut pinned = field_with(
            "Brim",
            checkbox("brim", false),
            FieldOpts::default().horizontal().control_width(140.0),
        );
        for root in [&mut inline, &mut pinned] {
            let mut state = crate::UiState::new();
            theme.apply_metrics(root);
            crate::layout::layout(root, &mut state, Rect::new(0.0, 0.0, 300.0, 60.0));
        }

        let sw = inline.children[1].computed_rect;
        let cb = pinned.children[1].computed_rect;
        assert!(sw.w < 48.0, "switch should not stretch, got {}", sw.w);
        assert!(cb.w < 24.0, "checkbox should not stretch, got {}", cb.w);
    }

    #[test]
    fn inline_label_rows_share_one_control_edge() {
        // The inspector promise: whatever the label, every control
        // starts at `gutter + SPACE_3` and runs to the pane's edge.
        const GUTTER: f32 = 72.0;
        let opts = FieldOpts::default().inline_label(GUTTER);
        let mut root = field_group([
            field_with(
                "Value",
                text_input("value", "10k", &Selection::default()),
                opts,
            ),
            field_with(
                "Footprint and package",
                select_trigger("footprint", "0603"),
                opts,
            ),
        ]);
        let mut state = crate::UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        let a = state.rect_of_key("value").expect("first control");
        let b = state.rect_of_key("footprint").expect("second control");
        assert!(
            (a.x - b.x).abs() < 0.5,
            "controls share a left edge, {} vs {}",
            a.x,
            b.x
        );
        assert!(
            (a.w - b.w).abs() < 0.5,
            "controls share a right edge, {} vs {}",
            a.x + a.w,
            b.x + b.w
        );
        let group = root.computed_rect;
        assert!(
            (a.x - (group.x + tokens::RING_WIDTH + GUTTER + tokens::SPACE_3)).abs() < 0.5,
            "control edge should sit at gutter + SPACE_3 inside the group's ring reserve, got {}",
            a.x
        );
    }

    #[test]
    fn field_group_uses_the_shadcn_group_gap() {
        // FieldGroup is `gap-7` — SPACE_7 on the damascene scale.
        let g = field_group([
            field("One", switch("a", false)),
            field_separator(),
            field("Two", switch("b", true)),
        ]);

        assert_eq!(g.axis, Axis::Column);
        assert_eq!(g.metrics_role, Some(MetricsRole::Form));
        assert_eq!(g.width, Size::Fill(1.0));
        assert_eq!(g.gap, tokens::SPACE_7);
        assert_eq!(g.padding, Sides::xy(tokens::RING_WIDTH, 0.0));
        assert_eq!(g.children.len(), 3);
        assert_eq!(g.children[1].kind, Kind::Divider);
    }

    #[test]
    fn field_set_places_a_medium_legend_over_the_group() {
        // FieldLegend's default tone is `text-base font-medium` with
        // `mb-3`; the fieldset stacks its fields at `gap-6`.
        let s = field_set("Motion & chrome", [field("One", switch("a", false))]);

        assert_eq!(s.axis, Axis::Column);
        assert_eq!(s.gap, tokens::SPACE_3);
        assert_eq!(s.children.len(), 2);

        let legend = &s.children[0];
        assert_eq!(legend.text.as_deref(), Some("Motion & chrome"));
        assert_eq!(legend.text_role, TextRole::Title);
        assert_eq!(legend.font_size, tokens::TEXT_BASE.size);
        assert_eq!(legend.font_weight, FontWeight::Medium);

        let group = &s.children[1];
        assert_eq!(group.axis, Axis::Column);
        assert_eq!(group.gap, tokens::SPACE_6);
        assert_eq!(group.children.len(), 1);
    }
}
