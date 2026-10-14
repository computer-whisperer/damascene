//! VS Code workbench vocabulary for Damascene.
//!
//! damascene-core speaks **shadcn/Tailwind**, which is a landing-page
//! and dashboard system. This crate is the *application shell* opinion:
//! the VS Code workbench's token vocabulary, its density, and its
//! layered surface model, applied to the stock widget set.
//!
//! Stock damascene stays shadcn. Nothing here replaces or patches the
//! default vocabulary — a `button()` still behaves exactly as it does
//! anywhere else, it is simply 28px, 1.4px-cornered, and painted from a
//! layered workbench palette. Forms, dialogs, buttons and controls keep
//! their shadcn anatomy; this crate resizes and squares them and adds the
//! four chrome widgets core lacks.
//!
//! The ratified design record is `docs/WORKBENCH_VISION.md` — including
//! the rejected alternatives (Carbon, Blender-as-skin, the
//! Tailwind-zinc devtool genre) with their reasons, and the 2026-07-27
//! retarget that moved the *values* to the stock slate + blue palette
//! while keeping VS Code's *keys* (§"Retarget ratified"; see
//! [`theme::theme`] and [`theme::dark_modern`]).
//!
//! # Why this exists
//!
//! Three real applications built carefully on stock damascene — an ECAD
//! workbench, a voice client, a slicer — consistently read as demos
//! rather than tools, without fighting the theme anywhere. The
//! diagnosis was four structural signals that landing-page design
//! maximizes and professional tools minimize:
//!
//! | signal | shadcn | workbench |
//! |---|---|---|
//! | control scale | 36px `Md` controls, 14px type | 28px `Xs` controls, 13px type |
//! | radius + shadow | 7–12px, floating shadows | ~2px, flat chrome (overlays keep `SHADOW_MD`) |
//! | surfaces | one bg, outlined cards floating on it | layered fills, 1px separators |
//! | whitespace | spent to look calm | spent on information |
//!
//! All four run through core theme knobs (`with_default_component_size`,
//! `with_radius_scale`, `with_shadow_scale`, `with_type_scale`, the
//! palette) — the crate contributes values and chrome recipes, no
//! mechanisms.
//!
//! ## The strips go one rung denser than the app baseline
//!
//! 28px `Xs` is the *app* density — forms, dialogs, editor content. It
//! does not fit the strips that frame them: a 22px status bar or pane
//! header, or a 30px title bar, which has to hold the control *and* its
//! focus ring. Core's ladder has a rung below shadcn's floor for exactly
//! this, [`damascene_core::metrics::ComponentSize::Xxs`] — a 22px
//! control — so a button or `icon_button` in a strip is
//! `.size(ComponentSize::Xxs)`, never a hardcoded `.height(...)`. Reach
//! for it per element, or per role via `ThemeMetrics::with_button_size`;
//! it is deliberately *not* this crate's theme-wide default, because an
//! app whose forms are all 22px reads as broken rather than dense.
//!
//! # Why VS Code specifically — the key vocabulary, not the colors
//!
//! Damascene's premise is vocabulary parity with the LLM training
//! distribution. VS Code's theme keys are the most heavily
//! corpus-represented dense-app token system in existence — hundreds of
//! thousands of published theme files name `sideBar.background`,
//! `statusBar.background`, `focusBorder` — so they are the name source
//! for chrome no other system supplies, and an agent told "put the
//! status bar on `statusBar.background`" needs no glossary.
//!
//! That argument was ratified for the names and **rejected for the
//! values** on 2026-07-27 (`docs/WORKBENCH_VISION.md`, §"Retarget
//! ratified"): four independent agents given only "dense professional
//! tool, VS Code workbench genre" all produced zinc-family neutrals with
//! a sparse blue accent rather than Dark Modern's ramp, and value-level
//! fidelity to VS Code was never load-bearing for the four inversions
//! above. So [`theme::theme`] paints the stock
//! `Palette::radix_slate_blue_dark` under the VS Code key names, and
//! Dark Modern's calibrated values live on in [`theme::dark_modern`].
//!
//! Not binding: VS Code's colors (see above), its DOM, its widget
//! internals, or its widget shapes where shadcn already has an anatomy.
//! A select is still a shadcn select, at workbench size and radius.
//!
//! # Usage
//!
//! ```no_run
//! use damascene_core::prelude::*;
//! use damascene_workbench::{chrome::*, theme, tokens as vs};
//!
//! struct Shell;
//!
//! impl App for Shell {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         column([
//!             row([
//!                 column([pane_header("EXPLORER", Vec::<El>::new())])
//!                     .width(Size::Fixed(220.0))
//!                     .height(Size::Fill(1.0))
//!                     .fill(vs::SIDE_BAR_BG)
//!                     .border_r()
//!                     .border_color(vs::SIDE_BAR_BORDER),
//!                 column([text("editor")])
//!                     .width(Size::Fill(1.0))
//!                     .height(Size::Fill(1.0))
//!                     .fill(vs::EDITOR_BG),
//!             ])
//!             .height(Size::Fill(1.0))
//!             .align(Align::Stretch),
//!             status_bar([text("main").caption()], [chip("2")]),
//!         ])
//!         .align(Align::Stretch)
//!     }
//!
//!     // Without this, stock controls render at shadcn's 36px on a flat
//!     // single-surface palette.
//!     fn theme(&self) -> Theme {
//!         theme::theme()
//!     }
//! }
//! ```
//!
//! Run the full example with
//! `cargo run -p damascene-workbench --example shell`, and the dense
//! data-table tour with
//! `cargo run -p damascene-workbench --example data_table` — four views
//! over `damascene_core::widgets::data_table` (plain, selectable +
//! sorted + detail panel, grouped, and 10k virtualized) at the
//! workbench's 29px row pitch.
//!
//! # Calibration
//!
//! **Color is calibrated; metrics are not.**
//! `references/vscode-calibration/` vendors VS Code's own Dark Modern
//! theme chain verbatim from a pinned upstream commit, and every color
//! in [`tokens`] cites its key and value. Since the 2026-07-27 retarget
//! those constants are the *variant's* values: they are what
//! [`theme::dark_modern`] paints, and what the default theme's keys are
//! remapped away from onto the stock palette's slots. The metric
//! constants (bar heights, chip height, the 2px radius target) are the
//! indicative figures from `docs/WORKBENCH_VISION.md` and remain
//! hypotheses until the screenshot-measurement step of that document's
//! calibration plan lands. They are marked individually in [`tokens`].
//!
//! # Status
//!
//! First cut. Present: the palette, the dense metrics profile, and the
//! chrome widgets core lacks. Deferred deliberately (see
//! `docs/WORKBENCH_VISION.md`, "Non-goals for the first cut"): a shared
//! split-tree. The other two deferrals have since been spent — the
//! stock tree view is `damascene_core::widgets::tree`, and `data_table`
//! landed in core rather than here (`docs/WORKBENCH_VISION.md`,
//! "`data_table` deferral lifted"), because a table's *behaviour* is
//! not one of the four style signals this crate inverts.

#![warn(missing_docs)]

pub mod chrome;
pub mod theme;
pub mod tokens;
