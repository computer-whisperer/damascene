//! The chrome widgets core lacks, styled from the workbench theme.
//!
//! Core already ships the shell widgets an application needs most —
//! `menubar`, `toolbar`, `editor_tabs`, `sidebar`, `resize_handle`,
//! `number_scrubber`. This module adds the ones it does not, in the
//! order `docs/WORKBENCH_VISION.md` names them: a status bar, a chip, a
//! pane header, and the hairline rule — plus, since the 2026-07-27
//! acceptance round measured them as the remaining shell hand-rolls, a
//! [`title_bar`], a [`section_header`], and [`vertical_hairline`].
//!
//! # Names come from the VS Code part vocabulary
//!
//! Per `docs/NAMING_ORACLE.md`, workbench chrome recipes take **VS Code
//! part names** — the only name source that exists for this layer.
//! [`status_bar`] is `statusbar`, [`title_bar`] is `titlebar`,
//! [`pane_header`] is `sideBarSectionHeader`. Where a recipe is not a
//! workbench part at all, its rustdoc says which oracle it does cite
//! (see [`section_header`]) or records that it has none.
//!
//! # Everything here is a stock `El`
//!
//! No new `Kind`, no new metrics role, no core changes. These are
//! recipes: `row`/`column` with workbench fills, per-side borders, and
//! the dense heights from [`crate::tokens`]. An app is free to inline
//! any of them and tune it.
//!
//! # Borders, not interleaved rules
//!
//! The bars here separate themselves with `.border_t()` / `.border_b()`
//! — real CSS-semantics per-side borders, which join padding in the
//! layout content inset. The retired `damascene-carbon` experiment had
//! to interleave 1px filled siblings for the same effect (its
//! `hairline()` had 12 call sites); that workaround is what
//! `docs/VOCABULARY_PARITY.md` §1 was written to delete.
//!
//! [`hairline`] survives anyway, because per-side borders paint on the
//! bordered element. A rule *between* two siblings that neither of them
//! should own — a separator inside a scrolling list, a divider between
//! toolbar groups — is still a node of its own.
//!
//! # Two of these are pure aliases
//!
//! [`hairline`] and [`vertical_hairline`] are one-call delegations to
//! core's `separator()` / `vertical_separator()`, and [`chip`] is
//! `badge()` at the chrome rung with the chrome material stamped on.
//! Where a workbench name and a stock widget mean the same object, the
//! name is the only thing this module adds — there is no second
//! implementation to drift.

#![warn(missing_docs)]

use damascene_core::metrics::ComponentSize;
use damascene_core::style::StyleProfile;
use damascene_core::tokens;
use damascene_core::tree::*;
use damascene_core::widgets::badge::badge;
use damascene_core::widgets::separator::{separator, vertical_separator};
use damascene_core::widgets::text::text;

use crate::tokens as vs;

/// The workbench status bar — a [`vs::STATUS_BAR_HEIGHT`] strip of
/// terse live readouts across the bottom of the shell.
///
/// # Anatomy
///
/// One `row`, filled [`vs::STATUS_BAR_BG`], with a top border in
/// [`vs::STATUS_BAR_BORDER`]:
///
/// ```text
/// row  fill=statusBar.background  border-t=statusBar.border  h=22
///   ├─ row   (leading items, gap SPACE_3)
///   ├─ spacer
///   └─ row   (trailing items, gap SPACE_3)
/// ```
///
/// The two item rows are always present, even when empty, so the
/// spacer's split point does not depend on the contents — an app can
/// hand either side an empty iterator without the other side moving.
///
/// Deliberately the shortest region in the shell: at 22px it reads as
/// instrumentation rather than as content. Text placed here should be
/// `.caption()` (12px); the bar sets [`vs::STATUS_BAR_FG`] as its text
/// color so plain `text()` children land on the right value.
pub fn status_bar<L, LE, T, TE>(leading: L, trailing: T) -> El
where
    L: IntoIterator<Item = LE>,
    LE: Into<El>,
    T: IntoIterator<Item = TE>,
    TE: Into<El>,
{
    let items = |children: Vec<El>| {
        row(children)
            .gap(tokens::SPACE_3)
            .align(Align::Center)
            .height(Size::Fill(1.0))
    };
    let leading: Vec<El> = leading.into_iter().map(Into::into).collect();
    let trailing: Vec<El> = trailing.into_iter().map(Into::into).collect();

    row([items(leading), spacer(), items(trailing)])
        .fill(vs::STATUS_BAR_BG)
        .text_color(vs::STATUS_BAR_FG)
        .border_t()
        .border_color(vs::STATUS_BAR_BORDER)
        .padding(Sides::x(tokens::SPACE_2))
        .height(Size::Fixed(vs::STATUS_BAR_HEIGHT))
        .width(Size::Fill(1.0))
        .align(Align::Center)
}

/// A dense status chip — VS Code's `badge`, at
/// [`vs::CHIP_HEIGHT`].
///
/// # Anatomy
///
/// ```text
/// chip = badge  size=Xxs  fill=badge  radius=2  profile=Solid
///        caption  regular  color=badge-foreground
/// ```
///
/// # It is the stock `badge`, restyled
///
/// One anatomy serves both: the body is `badge(label)` at
/// [`ComponentSize::Xxs`] — the *chrome rung* of the badge metrics
/// ladder, 16px tall with 4px of horizontal padding, which is exactly
/// this chip's geometry — with three chrome stamps on top. There is no
/// second pill implementation here, and [`vs::CHIP_HEIGHT`] is the
/// rung's height rather than a competing literal (asserted in this
/// module's tests).
///
/// The three stamps are the whole difference:
///
/// 1. **Material.** `.ghost()` strips the badge's tinted fill and
///    stroke, then a solid [`tokens::BADGE`] fill goes on with no
///    stroke at all. A stroke would straddle the boundary and widen the
///    chip by a pixel on every side; chrome wants a flat block.
/// 2. **Profile.** [`StyleProfile::Solid`] instead of the badge's
///    `Tinted`, which is what makes the status modifiers paint filled
///    chips (see below).
/// 3. **Radius.** Pinned to [`vs::RADIUS`] with `.radius()` rather than
///    left to the theme, so the theme's radius scale leaves it alone —
///    a chip stays at exactly [`vs::RADIUS`] under any scale, including
///    a consumer who squares the app entirely.
///
/// The label also keeps caption's regular weight where a badge goes
/// Medium: at 12px inside a 16px strip the extra weight thickens the
/// glyphs without buying legibility, and VS Code's own badges are set
/// at the normal face.
///
/// Reach for `badge()` when you want shadcn's tinted status pill on a
/// page; reach for `chip` for counts and flags packed into chrome.
///
/// # The material is the `badge` palette slot
///
/// `damascene_core::tokens::BADGE` — the **stock** token, the same way
/// [`hairline`] resolves to `tokens::BORDER`, so the chip follows a
/// palette swap in a downstream theme of this vocabulary. Both themes
/// in this crate point VS Code's `badge.background` at that same slot,
/// so the pixel is also `vs::BADGE_BG` either way.
///
/// This is a change from the crate's first cut, and the reason the slot
/// exists. `badge.background` used to be remapped onto `secondary`,
/// which is a *near-surface* value: a chip on a [`pane_header`]
/// measured 1.11:1 under the default theme — the fill was invisible and
/// only the label read. The dedicated slot is the material VS Code's
/// own `#616161` is, one that stands clear of every surface in the ramp
/// (3.4:1 over `card` under the default theme).
///
/// # Status tints
///
/// The profile is `StyleProfile::Solid`, so the stock status modifiers
/// produce **solid** chips with contrasting labels:
/// `chip("3").destructive()` is a filled red count, not a tinted
/// outline. That is the chrome idiom — VS Code's own remote indicator
/// is a solid `#0078D4` block with white text — and it is why there is
/// no `chip_with_color`: `.info()`, `.success()`, `.warning()`,
/// `.destructive()`, `.primary()` and `.muted()` already name every
/// tint a chip should have, and they cost zero new vocabulary.
///
/// A tinted chip is set **semibold**, because that is what the `Solid`
/// tint does to every widget that wears it (a filled button, a solid
/// badge) and the chip is now one node rather than a box around a text
/// leaf the modifier could not reach. The plain chip keeps the regular
/// face described above.
///
/// # Geometry comes from the ladder, not from here
///
/// Because the rung is what sizes it, a chip carries no explicit height
/// or padding: [`vs::CHIP_HEIGHT`] and `SPACE_1` land on it through the
/// metrics pass, the same way every other stock control takes its size.
/// A caller who needs a different one asks for a different rung
/// (`chip("3").size(ComponentSize::Xs)`) rather than hardcoding pixels.
pub fn chip(label: impl Into<String>) -> El {
    badge(label)
        .size(ComponentSize::Xxs)
        // Strips the tint's fill *and* stroke; the chrome material goes
        // on next, and nothing else may paint an edge.
        .ghost()
        .style_profile(StyleProfile::Solid)
        .fill(tokens::BADGE)
        .text_color(tokens::BADGE_FOREGROUND)
        .font_weight(FontWeight::Regular)
        .radius(vs::RADIUS)
}

/// The window title strip — VS Code's `titlebar`, at
/// [`vs::TITLE_BAR_HEIGHT`].
///
/// The top edge of a shell: app identity and the menus on the left, the
/// window's own actions on the right, flush against the frame.
///
/// # Anatomy
///
/// ```text
/// row  fill=titleBar.activeBackground  border-b=titleBar.border  h=30
///   ├─ row   (leading items, gap SPACE_3)
///   ├─ spacer
///   └─ row   (trailing items, gap SPACE_3)
/// ```
///
/// The mirror image of [`status_bar`] — same three-slot shape, same
/// gap rhythm, same always-present item rows so the spacer's split
/// point never depends on the contents — with the rule on the *bottom*
/// edge instead of the top, because the strip separates itself from
/// what is below it. The bar sets [`vs::TITLE_BAR_ACTIVE_FG`] as its
/// text color, so a plain `text()` child lands on the right value.
///
/// ```ignore
/// use damascene_core::prelude::*;
/// use damascene_workbench::chrome::*;
///
/// title_bar(
///     [
///         text("Meridian Slicer").caption().font_weight(FontWeight::Medium),
///         row(["File", "Edit", "View"].map(|label| {
///             menubar_trigger("menu", label.to_lowercase(), label, false)
///         }))
///         .gap(tokens::SPACE_0)
///         .align(Align::Center),
///     ],
///     [button("Slice").key("slice").primary()],
/// )
/// ```
///
/// Menus go in `leading` as one nested row at `SPACE_0`, the way every
/// shell in `examples/` builds them: the triggers abut each other and
/// the group spaces normally against the app name.
///
/// # Not core's `menubar()`
///
/// `damascene_core::menubar` is shadcn's `Menubar`: a **boxed floating
/// menu bar** — `RADIUS_MD` corners, a `border` stroke, a `background`
/// fill, `SPACE_1` of padding all round, hugging width — the component
/// you drop into a page that has margins. This is the opposite object:
/// a full-bleed 30px strip with no corners, no side padding beyond
/// `SPACE_2`, and an under-rule, flush to the window edge. Reach for
/// `menubar()` inside a document; reach for `title_bar` for the top
/// edge of a shell, and put core's `menubar_trigger`s straight into its
/// `leading` slot without the `menubar()` box around them.
pub fn title_bar<L, LE, T, TE>(leading: L, trailing: T) -> El
where
    L: IntoIterator<Item = LE>,
    LE: Into<El>,
    T: IntoIterator<Item = TE>,
    TE: Into<El>,
{
    let items = |children: Vec<El>| {
        row(children)
            .gap(tokens::SPACE_3)
            .align(Align::Center)
            .height(Size::Fill(1.0))
    };
    let leading: Vec<El> = leading.into_iter().map(Into::into).collect();
    let trailing: Vec<El> = trailing.into_iter().map(Into::into).collect();

    row([items(leading), spacer(), items(trailing)])
        .fill(vs::TITLE_BAR_ACTIVE_BG)
        .text_color(vs::TITLE_BAR_ACTIVE_FG)
        .border_b()
        .border_color(vs::TITLE_BAR_BORDER)
        .padding(Sides::x(tokens::SPACE_2))
        .height(Size::Fixed(vs::TITLE_BAR_HEIGHT))
        .width(Size::Fill(1.0))
        .align(Align::Center)
}

/// A compact pane title strip with a trailing action slot — VS Code's
/// `sideBarSectionHeader`.
///
/// # Anatomy
///
/// ```text
/// row  fill=sideBarSectionHeader.background
///      border-b=sideBarSectionHeader.border  h=22  px=SPACE_2
///   ├─ text   (title, caption, medium, color=sideBarSectionHeader.foreground)
///   ├─ spacer
///   └─ row    (trailing actions, gap SPACE_1)
/// ```
///
/// The trailing row is always present so the spacer's split point is
/// contents-independent, matching [`status_bar`].
///
/// At [`vs::PANE_HEADER_HEIGHT`] this is the *section header* rung, not
/// VS Code's taller editor group header — it is the strip that caps an
/// accordion section or a docked panel. For an editor group, use core's
/// `editor_tabs` on [`vs::EDITOR_GROUP_HEADER_TABS_BG`].
///
/// Not to be confused with [`section_header`], despite the VS Code key
/// this recipe is named for: `pane_header` is a *filled, ruled, fixed
/// height strip of chrome*; `section_header` is an unfilled title +
/// caption stack inside a settings page's content column.
///
/// The strip is 22px, so the trailing actions have to be too: give each
/// one `.size(ComponentSize::Xxs)`
/// ([`damascene_core::metrics::ComponentSize::Xxs`], the chrome rung),
/// which stamps a 22px `icon_button` — exactly the header height. The
/// stock default (`Md`, 36px) and even `Xs` (28px) overflow this strip,
/// and hardcoding `.height(22.0)` loses the matching radius, padding,
/// and gap the rung carries with it.
///
/// ```no_run
/// # use damascene_core::prelude::*;
/// # use damascene_workbench::chrome::pane_header;
/// pane_header(
///     "OUTLINE",
///     [
///         icon_button("eye").ghost().size(ComponentSize::Xxs),
///         icon_button("more-horizontal").ghost().size(ComponentSize::Xxs),
///     ],
/// );
/// ```
pub fn pane_header<I, E>(title: impl Into<String>, trailing: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let trailing: Vec<El> = trailing.into_iter().map(Into::into).collect();

    row([
        text(title)
            .caption()
            .font_weight(FontWeight::Medium)
            .text_color(vs::SIDE_BAR_SECTION_HEADER_FG)
            .ellipsis(),
        spacer(),
        row(trailing).gap(tokens::SPACE_1).align(Align::Center),
    ])
    .fill(vs::SIDE_BAR_SECTION_HEADER_BG)
    .border_b()
    .border_color(vs::SIDE_BAR_SECTION_HEADER_BORDER)
    .padding(Sides::x(tokens::SPACE_2))
    .height(Size::Fixed(vs::PANE_HEADER_HEIGHT))
    .width(Size::Fill(1.0))
    .align(Align::Center)
}

/// A settings-page section heading — a title over a dim one-line
/// caption.
///
/// # Anatomy
///
/// ```text
/// column  gap=SPACE_1  w=Fill  h=Hug
///   ├─ text  title    (TEXT_BASE, semibold, foreground)
///   └─ text  caption  (TEXT_XS, muted-foreground, wrapping, fill-width)
/// ```
///
/// # Oracle
///
/// Not a VS Code part: nothing in the workbench chrome vocabulary names
/// this, because in VS Code it is a *document* heading inside the
/// settings editor rather than a region of the shell. Its anatomy comes
/// from the widget oracle instead — shadcn's `CardHeader` /
/// `CardTitle` / `CardDescription`, which core already ships as
/// `card_header([card_title(..), card_description(..)])`. This is that
/// stack **without the card**: a settings page groups its rows by
/// heading and hairline, not by boxing each group in a panel, so
/// `card()`'s fill, stroke and padding would draw a container the
/// design does not have.
///
/// See [`pane_header`] for the other thing "section header" can mean —
/// the filled, ruled, 22px `sideBarSectionHeader` strip. That one is
/// chrome; this one is content.
///
/// # Why `.title()` and not `.heading()`
///
/// Under this crate's [`crate::theme::TYPE_SCALE`] the Title role lands
/// at ~15px, which is the size a dense settings page wants for a group
/// heading. The Heading role is `TEXT_2XL` — ~22px here — and reads as
/// a marketing section, which is the note `examples/settings.rs` left
/// when it hand-rolled this shape and reached for `.label()` +
/// `Semibold` to avoid the jump. The stock role that means "section
/// title" is Title; the ladder gap is a core observation, not a reason
/// for every settings page to re-derive its own heading size.
pub fn section_header(title: impl Into<String>, description: impl Into<String>) -> El {
    column([
        text(title).title(),
        text(description).caption().muted().wrap_text().fill_width(),
    ])
    .gap(tokens::SPACE_1)
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

/// A 1px horizontal rule in `tokens::BORDER` — the workbench name for
/// core's [`separator`].
///
/// # It *is* `separator()`
///
/// The body is one call: this recipe adds nothing to the stock widget
/// and exists only so a workbench shell can say "hairline", the word
/// the VS Code vocabulary uses for a region rule, without leaving the
/// name unaccounted for. `separator()` is the shadcn name for the same
/// [`vs::HAIRLINE`]-thick node; reach for either. The pair of names is
/// the whole delta, so nothing can drift between them.
///
/// It resolves to the same value as [`vs::PANEL_BORDER`] under both of
/// this crate's themes — `#363A3F` under [`crate::theme::theme`],
/// `#2B2B2B` (`panel.border`) under [`crate::theme::dark_modern`] — and
/// is written as the *stock* token rather than the workbench one so the
/// rule follows a palette swap in a downstream theme of this vocabulary.
///
/// Per-side borders make this unnecessary for the common cases — a bar
/// separating itself from what is above or below it should use
/// `.border_t()` / `.border_b()`, which cost no node and no layout row.
/// `hairline` is for the case per-side borders cannot express: a rule
/// laid *between* siblings that neither sibling owns.
///
/// # It needs a stretching parent
///
/// The width is `Size::Fill(1.0)`, which is a *cross-axis* size inside
/// a `column` — so it resolves against the parent's [`Align`], not
/// against the parent's width. In a column aligned `Stretch` the rule
/// spans the content box; in one aligned `Start`/`Center`/`End` it
/// resolves to **zero width and paints nothing** (measured: rect
/// `(20,35,0,1)` vs `(20,35,360,1)` for the same tree at the two
/// alignments). A content column that hugs its children — the usual
/// settings-page shape — therefore has to either align `Stretch` or
/// give the rule an explicit `.width(...)`. The same applies to core's
/// `separator()`, and to [`vertical_hairline`] on the other axis.
pub fn hairline() -> El {
    separator()
}

/// [`hairline`] turned on its side — the workbench name for core's
/// [`vertical_separator`], a 1px *vertical* rule in `tokens::BORDER`
/// for the gap between two groups of controls inside one bar.
///
/// As with [`hairline`], the body is one call and the name is the whole
/// delta: `vertical_separator()` is the same node under the shadcn
/// vocabulary.
///
/// The same argument as [`hairline`], one axis over. Per-side borders
/// separate a bar from what is above or below it at no cost; they
/// cannot draw a rule *between* two toolbar groups, because neither
/// group owns it. That is this node's whole job, and it is the shape
/// `examples/parts.rs` and `examples/slicer_match.rs` both hand-rolled.
///
/// # Height — usually set it
///
/// `Size::Fill(1.0)`, mirroring [`hairline`]'s full width, and subject
/// to the same rule: height is the *cross-axis* size inside a `row`, so
/// it resolves against the parent's [`Align`]. Only a bar aligned
/// `Stretch` gives the rule the bar's full content height; a bar that
/// centers its items — which is what [`status_bar`], [`title_bar`] and
/// core's `toolbar` all do — collapses it to **zero height and paints
/// nothing**.
///
/// So in practice a toolbar rule takes an explicit height, and wants
/// one anyway: `vertical_hairline().height(Size::Fixed(18.0))` is the
/// inset rule both `examples/parts.rs` and `examples/slicer_match.rs`
/// drew by hand, short enough to read as a group divider rather than as
/// a region boundary.
pub fn vertical_hairline() -> El {
    vertical_separator()
}

#[cfg(test)]
mod tests {
    use super::*;
    use damascene_core::theme::palette::Palette;

    /// WCAG 2.x relative luminance of an opaque sRGB color.
    fn luminance(c: Color) -> f32 {
        let [r, g, b, _] = c.to_srgb_u8a();
        let lin = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    }

    /// WCAG 2.x contrast ratio between two opaque sRGB colors.
    fn contrast(a: Color, b: Color) -> f32 {
        let (mut hi, mut lo) = (luminance(a), luminance(b));
        if hi < lo {
            std::mem::swap(&mut hi, &mut lo);
        }
        (hi + 0.05) / (lo + 0.05)
    }

    /// Both palettes this crate ships, each with the contrast floor its
    /// chip *material* is held to, named for assertion messages.
    ///
    /// The default theme gets 3:1 — the WCAG non-text floor, and what
    /// its `badge` slot is solved for (measured 3.43:1 over `card`).
    /// [`crate::theme::dark_modern`] gets 2.6, because it
    /// **transcribes** VS Code's own `badge.background`: upstream,
    /// `#616161` measures 2.87:1 over the `#181818` chrome and 2.66:1
    /// over the `#1F1F1F` editor. Holding a transcription to our floor
    /// would mean inventing a value, which
    /// `docs/WORKBENCH_VISION.md`'s calibration plan forbids — the
    /// reference wins, and the looser floor records exactly how much
    /// it wins by.
    fn palettes() -> [(&'static str, Palette, f32); 2] {
        [
            ("theme (slate)", crate::theme::slate_palette(), 3.0),
            ("dark_modern", crate::theme::dark_modern_palette(), 2.6),
        ]
    }

    #[test]
    fn status_bar_has_the_documented_anatomy() {
        let bar = status_bar([chip("2")], [text("Ln 1, Col 1")]);

        assert_eq!(bar.height, Size::Fixed(vs::STATUS_BAR_HEIGHT));
        assert_eq!(bar.fill, Some(vs::STATUS_BAR_BG));
        assert_eq!(bar.children.len(), 3, "leading / spacer / trailing");

        let border = bar.border.as_ref().expect("status bar borders itself");
        assert_eq!(border.widths.top, 1.0, "over-rule above the bar");
        assert_eq!(border.widths.bottom, 0.0);
        assert_eq!(border.color, Some(vs::STATUS_BAR_BORDER));

        assert_eq!(bar.children[0].children.len(), 1);
        assert_eq!(bar.children[2].children.len(), 1);
    }

    #[test]
    fn status_bar_item_rows_survive_empty_sides() {
        // The split point must not depend on the contents.
        let bar = status_bar(Vec::<El>::new(), Vec::<El>::new());
        assert_eq!(bar.children.len(), 3);
        assert!(bar.children[0].children.is_empty());
        assert!(bar.children[2].children.is_empty());
    }

    /// Run the metrics + layout passes under this crate's theme, the
    /// way every backend does before painting, and hand back the node.
    /// A chip takes its box from the badge ladder's chrome rung, so its
    /// geometry only exists after this.
    fn measured(el: El) -> El {
        use damascene_core::bundle::artifact::render_bundle_themed;
        let mut root = row([el])
            .align(Align::Center)
            .padding(0.0)
            .width(Size::Fill(1.0))
            .height(Size::Fixed(40.0));
        render_bundle_themed(
            &mut root,
            Rect::new(0.0, 0.0, 400.0, 40.0),
            &crate::theme::theme(),
        );
        root.children.remove(0)
    }

    #[test]
    fn chip_is_the_stock_badge_at_the_chrome_rung() {
        // The dedup, pinned: chip is `badge()` — same kind, same
        // metrics role — asking for the ladder's floor rung rather than
        // re-deriving a box of its own.
        let c = chip("3");
        assert_eq!(c.kind, badge("3").kind);
        assert_eq!(c.metrics_role, badge("3").metrics_role);
        assert_eq!(c.component_size, Some(ComponentSize::Xxs));
        assert_eq!(c.text.as_deref(), Some("3"));
        assert_eq!(c.width, Size::Hug);
        assert_eq!(c.fill, Some(tokens::BADGE));
        assert_eq!(c.radius.tl, vs::RADIUS);
        assert!(
            c.stroke.is_none(),
            "a chrome chip is a flat block — a stroke would straddle its \
             boundary and widen it"
        );
    }

    #[test]
    fn the_chrome_rung_is_the_chip_height_this_crate_names() {
        // `vs::CHIP_HEIGHT` is a *name* for the rung, not a second
        // source of truth. If core re-tunes `ComponentSize::Xxs`, this
        // fails rather than letting the two silently fork.
        assert_eq!(measured(chip("3")).computed_rect.h, vs::CHIP_HEIGHT);
        // And the horizontal padding the rung carries with it, which is
        // the `SPACE_1` the recipe used to stamp by hand.
        assert_eq!(measured(chip("3")).padding.left, tokens::SPACE_1);
        assert_eq!(measured(chip("3")).padding.right, tokens::SPACE_1);
    }

    #[test]
    fn chip_is_denser_and_squarer_than_the_stock_badge() {
        use damascene_core::widgets::badge::BADGE_RADIUS;

        // Measured against the real badge rather than against recalled
        // numbers: shorter than the badge's default rung (the ladder
        // runs 16/18/20/24/28) and squarer than its 6px corner.
        let c = measured(chip("3"));
        let b = measured(badge("3"));
        assert!(c.computed_rect.h < b.computed_rect.h);
        assert!(c.radius.tl < BADGE_RADIUS);
        assert!(b.fill.is_some());
    }

    #[test]
    fn chip_radius_is_explicit_so_the_radius_scale_leaves_it_alone() {
        // If this flag ever stops being set, a consumer squaring the app
        // with `with_radius_scale(0.0)` would flatten chips too.
        assert_eq!(chip("3").radius_origin, RadiusOrigin::Fixed);
    }

    #[test]
    fn chip_paints_the_stock_badge_slot_not_the_workbench_key() {
        // Same reasoning as `hairline`: the *stock* token, so the chip
        // follows a palette swap in a downstream theme of this
        // vocabulary. Only the token name distinguishes the two, and it
        // is the token that a palette re-resolves.
        let c = chip("3");
        assert_eq!(c.fill.and_then(|f| f.token), Some("badge"));
        assert_eq!(c.text_color.and_then(|f| f.token), Some("badge-foreground"));
    }

    #[test]
    fn a_chip_on_a_pane_header_is_a_readable_object_under_both_themes() {
        // The 2026-07-27 acceptance complaint, pinned at the value
        // level. Before the `badge` slot existed, `badge.background`
        // was remapped onto `secondary` and this measured **1.11:1**
        // under the default theme — an invisible chip carrying a
        // readable label.
        for (name, p, floor) in palettes() {
            let fill = p.resolve(chip("3").fill.expect("a chip is filled"));
            let header = p.resolve(
                pane_header("EXPLORER", Vec::<El>::new())
                    .fill
                    .expect("a pane header is filled"),
            );
            let ratio = contrast(fill, header);
            assert!(
                ratio >= floor,
                "{name}: chip on pane_header is {ratio:.2}:1"
            );

            // And the label still reads on the chip it sits in — this
            // one is the real small-text floor under both themes.
            let label = p.resolve(chip("3").text_color.expect("the label is colored"));
            let text_ratio = contrast(label, fill);
            assert!(
                text_ratio >= 4.5,
                "{name}: chip label is {text_ratio:.2}:1, under the 4.5:1 floor"
            );
        }
    }

    #[test]
    fn the_default_theme_clears_the_full_non_text_floor_for_chips() {
        // Held to 3:1 proper, not the transcription-tolerant floor:
        // the slate values are *ours*, so there is nothing upstream to
        // defer to. Measured 3.43:1 (slate-9 over the `card` step).
        let p = crate::theme::slate_palette();
        let fill = p.resolve(chip("3").fill.unwrap());
        let header = p.resolve(pane_header("X", Vec::<El>::new()).fill.unwrap());
        let ratio = contrast(fill, header);
        assert!(ratio >= 3.0, "chip on pane_header is {ratio:.2}:1");
    }

    #[test]
    fn a_chip_reads_on_every_ground_the_shell_puts_it_on() {
        // Chips land in the status bar, on pane headers, on the tab
        // strip and in the content well. Those collapse onto one of two
        // slots under either theme, but assert the whole set so a
        // future re-slotting of a bar cannot quietly hide chips.
        for (name, p, floor) in palettes() {
            let fill = p.resolve(chip("3").fill.unwrap());
            for ground in [
                vs::STATUS_BAR_BG,
                vs::SIDE_BAR_SECTION_HEADER_BG,
                vs::TITLE_BAR_ACTIVE_BG,
                vs::EDITOR_GROUP_HEADER_TABS_BG,
                vs::EDITOR_BG,
            ] {
                let ratio = contrast(fill, p.resolve(ground));
                assert!(
                    ratio >= floor,
                    "{name}: chip on {} is {ratio:.2}:1",
                    ground.token.unwrap_or("?"),
                );
            }
        }
    }

    #[test]
    fn chip_status_modifiers_produce_solid_tints() {
        // Zero new names: the stock status vocabulary is the chip's
        // color API. `Solid` (not `Tinted`) is what makes
        // `chip("2").destructive()` a filled red block with a
        // contrasting label, the chrome idiom, rather than a badge's
        // tinted outline.
        assert_eq!(chip("2").style_profile, StyleProfile::Solid);

        for (c, tint) in [
            (chip("2").info(), tokens::INFO),
            (chip("2").success(), tokens::SUCCESS),
            (chip("2").warning(), tokens::WARNING),
            (chip("2").destructive(), tokens::DESTRUCTIVE),
        ] {
            assert_eq!(c.fill, Some(tint), "a tinted chip fills solid");
            // The label follows onto the contrasting foreground —
            // otherwise the caption keeps `badge-foreground` over a
            // colored fill.
            assert_ne!(
                c.text_color,
                Some(tokens::BADGE_FOREGROUND),
                "the label must leave the badge slot when the chip is tinted"
            );
            // Geometry is the shared chip recipe — tint changes color
            // only.
            assert_eq!(measured(c.clone()).computed_rect.h, vs::CHIP_HEIGHT);
            assert_eq!(c.radius.tl, vs::RADIUS);
            assert_eq!(c.width, Size::Hug);
        }

        // The plain chip is set at caption's regular face; a tinted one
        // goes semibold, because that is what the `Solid` tint does to
        // everything that wears it. Deliberate, and only reachable now
        // that the chip is one node instead of a box around a text leaf
        // the modifier could not touch.
        assert_eq!(chip("2").font_weight, FontWeight::Regular);
        assert_eq!(chip("2").destructive().font_weight, FontWeight::Semibold);

        // `.muted()` is the quiet rung: a neutral surface, not a tint.
        let m = chip("2").muted();
        assert_eq!(m.fill, Some(tokens::MUTED));
        assert_eq!(m.text_color, Some(tokens::MUTED_FOREGROUND));
    }

    #[test]
    fn tinted_chip_labels_are_readable_under_both_themes() {
        // `text_on_solid` picks the label from the tint's paired
        // `*-foreground` slot; assert the pairing actually lands rather
        // than trusting the mapping.
        //
        // The floor is 3:1, not the 4.5:1 the *untinted* chip clears,
        // and the gap is a **palette** property rather than a chip
        // decision: a status tint is `X` over `X-foreground`, the same
        // pair `button(..).info()` paints. Measured over the two stock
        // status pairs (2026-07-27) — slate: info/primary 3.26,
        // destructive 3.91, success 5.86, warning 7.21; Dark Modern:
        // destructive 3.35, success 3.37, info/primary 4.53, warning
        // 9.54. Both palettes put their step-9 blues and reds under 4.5
        // with white, which is Radix's documented step-9 behaviour and
        // VS Code's `#0078D4`. Raising it is a palette question for the
        // whole `*-foreground` family, not something to special-case
        // here; this assertion is the tripwire if any pair regresses
        // below the non-text floor.
        for (name, p, _material_floor) in palettes() {
            for c in [
                chip("2").info(),
                chip("2").success(),
                chip("2").warning(),
                chip("2").destructive(),
            ] {
                let fill = p.resolve(c.fill.unwrap());
                let label = p.resolve(c.text_color.unwrap());
                let ratio = contrast(label, fill);
                assert!(
                    ratio >= 3.0,
                    "{name}: tinted chip {:?} label is {ratio:.2}:1",
                    c.fill.and_then(|f| f.token),
                );
            }
        }
    }

    #[test]
    fn pane_header_has_the_documented_anatomy() {
        let h = pane_header("EXPLORER", [text("+")]);

        assert_eq!(h.height, Size::Fixed(vs::PANE_HEADER_HEIGHT));
        assert_eq!(h.fill, Some(vs::SIDE_BAR_SECTION_HEADER_BG));
        assert_eq!(h.children.len(), 3, "title / spacer / trailing");
        assert_eq!(h.children[0].text.as_deref(), Some("EXPLORER"));
        assert_eq!(h.children[2].children.len(), 1);

        let border = h.border.as_ref().expect("pane header borders itself");
        assert_eq!(border.widths.bottom, 1.0, "under-rule below the strip");
        assert_eq!(border.widths.top, 0.0);
        assert_eq!(border.color, Some(vs::SIDE_BAR_SECTION_HEADER_BORDER));
    }

    #[test]
    fn pane_header_keeps_its_trailing_slot_when_empty() {
        let h = pane_header("OUTLINE", Vec::<El>::new());
        assert_eq!(h.children.len(), 3);
        assert!(h.children[2].children.is_empty());
    }

    #[test]
    fn title_bar_has_the_documented_anatomy() {
        let bar = title_bar([text("Slicer").caption()], [text("Slice").caption()]);

        assert_eq!(bar.height, Size::Fixed(vs::TITLE_BAR_HEIGHT));
        assert_eq!(bar.fill, Some(vs::TITLE_BAR_ACTIVE_BG));
        assert_eq!(bar.text_color, Some(vs::TITLE_BAR_ACTIVE_FG));
        assert_eq!(bar.children.len(), 3, "leading / spacer / trailing");

        let border = bar.border.as_ref().expect("title bar borders itself");
        assert_eq!(border.widths.bottom, 1.0, "under-rule below the strip");
        assert_eq!(border.widths.top, 0.0);
        assert_eq!(border.color, Some(vs::TITLE_BAR_BORDER));

        assert_eq!(bar.children[0].children.len(), 1);
        assert_eq!(bar.children[2].children.len(), 1);
    }

    #[test]
    fn title_bar_item_rows_survive_empty_sides() {
        // Same contents-independent split point as `status_bar`.
        let bar = title_bar(Vec::<El>::new(), Vec::<El>::new());
        assert_eq!(bar.children.len(), 3);
        assert!(bar.children[0].children.is_empty());
        assert!(bar.children[2].children.is_empty());
    }

    #[test]
    fn title_bar_and_status_bar_are_mirror_images() {
        // The pair is the top and bottom edge of one shell. If their
        // rhythm or slot shape forks, a shell stops reading as one
        // object.
        let top = title_bar([text("a").caption()], [text("b").caption()]);
        let bottom = status_bar([text("a").caption()], [text("b").caption()]);

        assert_eq!(top.children.len(), bottom.children.len());
        assert_eq!(top.padding, bottom.padding);
        assert_eq!(top.align, bottom.align);
        assert_eq!(top.width, bottom.width);
        assert_eq!(top.children[0].gap, bottom.children[0].gap);

        // The rules are on opposite edges: the title bar separates
        // itself from what is below, the status bar from what is above.
        let edges = |e: &El| {
            let b = e.border.as_ref().expect("bordered");
            (b.widths.top, b.widths.bottom)
        };
        assert_eq!(edges(&top), (0.0, 1.0));
        assert_eq!(edges(&bottom), (1.0, 0.0));
    }

    #[test]
    fn title_bar_is_flush_where_core_menubar_is_boxed() {
        // The cross-reference, asserted: these are opposite objects and
        // the rustdoc on both sides says so. If `title_bar` ever grows
        // corners or a hugging width, the distinction is gone.
        use damascene_core::widgets::menubar::menubar;

        let strip = title_bar(Vec::<El>::new(), Vec::<El>::new());
        assert_eq!(strip.width, Size::Fill(1.0), "the strip is full-bleed");
        assert_eq!(strip.radius, Corners::all(0.0));
        assert!(strip.stroke.is_none(), "no box around a window edge");

        let boxed = menubar(Vec::<El>::new());
        assert_eq!(boxed.width, Size::Hug);
        assert!(boxed.radius.tl > 0.0);
        assert!(boxed.stroke.is_some());
    }

    #[test]
    fn section_header_stacks_a_title_over_a_dim_caption() {
        let h = section_header("Appearance", "How the workbench looks.");

        assert_eq!(h.axis, Axis::Column);
        assert_eq!(h.children.len(), 2);
        assert_eq!(h.gap, tokens::SPACE_1);
        assert_eq!(h.width, Size::Fill(1.0));
        assert_eq!(h.height, Size::Hug);

        let title = &h.children[0];
        assert_eq!(title.text.as_deref(), Some("Appearance"));
        assert_eq!(title.text_role, TextRole::Title);
        assert_eq!(title.font_weight, FontWeight::Semibold);

        let caption = &h.children[1];
        assert_eq!(caption.text.as_deref(), Some("How the workbench looks."));
        assert_eq!(caption.text_role, TextRole::Caption);
        assert_eq!(caption.text_color, Some(tokens::MUTED_FOREGROUND));
    }

    #[test]
    fn section_header_is_content_not_chrome() {
        // The distinction from `pane_header`, which shares the phrase
        // "section header" via VS Code's key name. This one paints no
        // ground, draws no rule, and is not a fixed-height strip.
        let h = section_header("Appearance", "…");
        assert!(h.fill.is_none());
        assert!(h.border.is_none());
        assert_eq!(h.height, Size::Hug);

        let strip = pane_header("EXPLORER", Vec::<El>::new());
        assert!(strip.fill.is_some());
        assert!(strip.border.is_some());
        assert_eq!(strip.height, Size::Fixed(vs::PANE_HEADER_HEIGHT));
    }

    #[test]
    fn section_header_description_wraps_instead_of_clipping() {
        // A one-liner that is not a one-liner at a narrow measure must
        // wrap; a settings page's content column is not always wide.
        let h = section_header("Editor", "A description long enough to need a second line.");
        assert_eq!(h.children[1].text_wrap, TextWrap::Wrap);
        assert_eq!(h.children[1].width, Size::Fill(1.0));
    }

    #[test]
    fn hairline_is_one_pixel_of_the_stock_border_token() {
        let r = hairline();
        assert_eq!(r.height, Size::Fixed(vs::HAIRLINE));
        assert_eq!(r.width, Size::Fill(1.0));
        // The *stock* token, so a palette swap moves it.
        assert_eq!(r.fill.and_then(|c| c.token), Some("border"));
    }

    #[test]
    fn the_hairlines_are_the_stock_separators_under_another_name() {
        // The dedup, pinned on both axes: these recipes delegate, so
        // the two vocabularies cannot describe two different rules. If
        // core re-tunes `separator()`, the workbench name follows
        // instead of forking.
        use damascene_core::widgets::separator::{separator, vertical_separator};

        for (alias, stock) in [
            (hairline(), separator()),
            (vertical_hairline(), vertical_separator()),
        ] {
            assert_eq!(alias.kind, stock.kind);
            assert_eq!(alias.width, stock.width);
            assert_eq!(alias.height, stock.height);
            assert_eq!(alias.fill, stock.fill);
        }
        // And the thickness both land on is the one this crate names.
        assert_eq!(hairline().height, Size::Fixed(vs::HAIRLINE));
        assert_eq!(vertical_hairline().width, Size::Fixed(vs::HAIRLINE));
    }

    #[test]
    fn vertical_hairline_is_hairline_turned_on_its_side() {
        let h = hairline();
        let v = vertical_hairline();

        assert_eq!(v.kind, h.kind);
        assert_eq!(v.fill, h.fill);
        // Axes swapped, nothing else.
        assert_eq!(v.width, Size::Fixed(vs::HAIRLINE));
        assert_eq!(v.height, Size::Fill(1.0));
        assert_eq!(h.width, Size::Fill(1.0));
        assert_eq!(h.height, Size::Fixed(vs::HAIRLINE));
        // The *stock* token, so a palette swap moves it.
        assert_eq!(v.fill.and_then(|c| c.token), Some("border"));
    }

    #[test]
    fn a_fill_sized_rule_needs_a_stretching_parent() {
        // Both rules size themselves `Fill` on their *cross* axis, so
        // the parent's `Align` — not its size — decides whether they
        // paint at all. Documented on both recipes; measured here so
        // the doc cannot rot into a wrong claim.
        use damascene_core::bundle::artifact::render_bundle_themed;
        use damascene_core::bundle::lint::FindingKind;

        let laid_out = |align: Align| {
            let mut root = column([text("a").caption(), hairline(), text("b").caption()])
                .align(align)
                .padding(20.0)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0));
            let bundle = render_bundle_themed(
                &mut root,
                Rect::new(0.0, 0.0, 400.0, 200.0),
                &crate::theme::theme(),
            );
            (root, bundle)
        };

        let collapse_findings = |b: &damascene_core::bundle::artifact::Bundle| {
            b.lint
                .findings
                .iter()
                .filter(|f| f.kind == FindingKind::CollapsedFillCrossAxis)
                .count()
        };

        let (stretched, stretched_bundle) = laid_out(Align::Stretch);
        let dump = damascene_core::bundle::inspect::dump_tree(
            &stretched,
            &damascene_core::state::UiState::default(),
        );
        assert!(
            dump.contains("rect=(20,35,360,1)"),
            "a stretched column gives the rule the content box:\n{dump}"
        );
        assert_eq!(
            collapse_findings(&stretched_bundle),
            0,
            "a painting rule is not a lint finding:\n{}",
            stretched_bundle.lint.text()
        );

        let (started, started_bundle) = laid_out(Align::Start);
        let dump = damascene_core::bundle::inspect::dump_tree(
            &started,
            &damascene_core::state::UiState::default(),
        );
        assert!(
            dump.contains("rect=(20,35,0,1)"),
            "a hugging column collapses the rule to zero width:\n{dump}"
        );
        // The silent half of the failure is what the lint exists for:
        // nothing overflows and nothing errors, so without a finding
        // the only signal is a rule that isn't there.
        assert_eq!(
            collapse_findings(&started_bundle),
            1,
            "the collapsed rule must be reported:\n{}",
            started_bundle.lint.text()
        );
    }

    #[test]
    fn vertical_hairline_takes_an_explicit_height_for_a_centered_bar() {
        // The documented escape hatch: a bar that centers its items
        // sizes a Fill child to its own content, so an inset rule needs
        // a height. Nothing in the recipe may block that.
        let v = vertical_hairline().height(Size::Fixed(18.0));
        assert_eq!(v.height, Size::Fixed(18.0));
        assert_eq!(v.width, Size::Fixed(vs::HAIRLINE));
    }

    #[test]
    fn hairline_and_panel_border_are_one_value_under_both_themes() {
        // The rule paints the *stock* token while the bars around it
        // paint `panel.border`. If the two ever diverge, every region
        // separator in the crate stops matching the rules between them —
        // so assert it on both palettes rather than on Dark Modern's
        // alone, where the mapping happens to be an identity.
        for p in [crate::theme::dark_modern_palette(), crate::theme::slate_palette()] {
            let fill = hairline().fill.expect("hairline is filled");
            let rule = p.resolve(fill);
            let border = p.resolve(vs::PANEL_BORDER);
            assert_eq!((rule.r, rule.g, rule.b), (border.r, border.g, border.b));
        }
        // And under Dark Modern it is still VS Code's own hairline.
        let dm = crate::theme::dark_modern_palette().resolve(hairline().fill.unwrap());
        assert_eq!(
            (dm.r, dm.g, dm.b),
            (vs::PANEL_BORDER.r, vs::PANEL_BORDER.g, vs::PANEL_BORDER.b)
        );
    }
}
