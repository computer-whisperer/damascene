//! Carbon Design System vocabulary for Damascene.
//!
//! damascene-core speaks **shadcn/Tailwind** — a content-site system.
//! This crate adds a second, parallel vocabulary that speaks **Carbon
//! (IBM)**, the design system built for dense enterprise application
//! UIs, plus **VS Code workbench** names for the shell anatomy Carbon
//! does not cover.
//!
//! Both vocabularies stay available. Nothing here replaces or patches
//! the stock set; a `button()` still behaves exactly as it does anywhere
//! else, and [`theme`] keeps its colors in step with the Carbon
//! surfaces around it.
//!
//! # Why a parallel set rather than restyled defaults
//!
//! Damascene's premise is vocabulary parity with what an LLM was trained
//! on. shadcn is the right target for a marketing page, a settings form,
//! or a CRUD dashboard. It is the wrong target for a trace viewer or a
//! layout editor: `card == background` in the stock dark palette, so a
//! surface is only visible as its outline; `card_content` pads at 24px;
//! the type scale bottoms out at 12px; and `RADIUS_LG` is 12px.
//!
//! Carbon answers all four, and it answers them with names an agent
//! already knows:
//!
//! | Need | Carbon | damascene-core |
//! |---|---|---|
//! | Nested surfaces | `layer-01/02/03` ramp | `card`, `== background` |
//! | Dense spacing | `spacing-01` = 2px | `SPACE_1` = 4px |
//! | Dense type | `body-compact-01` 14/18 | `TEXT_SM` 14/20, floor 12 |
//! | Corners | square | `RADIUS_LG` = 12px |
//!
//! # Usage
//!
//! ```no_run
//! use damascene_core::prelude::*;
//! use damascene_carbon::{theme, widgets::*};
//!
//! struct Inspector;
//!
//! impl App for Inspector {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         column([
//!             ui_shell_header("trace inspector", Vec::<El>::new()),
//!             pane(
//!                 pane_header(IconName::Activity, "Frame 1042", Vec::<El>::new()),
//!                 [structured_list([prop_row("Draw calls", numeric("812"))])],
//!             ),
//!         ])
//!     }
//!
//!     // Without this, stock controls render in shadcn zinc on Carbon gray.
//!     fn theme(&self) -> Theme {
//!         theme::theme()
//!     }
//! }
//! ```
//!
//! # Status
//!
//! Built as an experiment in the landing-page-vs-application split. It
//! needs **no changes to damascene-core** — every extension point it
//! relies on is already public. Four gaps showed up while writing it,
//! each an additive core change that would make this crate smaller:
//!
//! 1. **No per-side stroke.** There is no `border-b`, one of the most
//!    used Tailwind utilities. Every bar and table row here pays an
//!    extra [`widgets::hairline`] node for a bottom rule.
//! 2. **Radius is not derived from a base.** `RADIUS_SM/MD/LG` are hard
//!    constants; shadcn itself derives them from one `--radius` with
//!    `calc()`, so core is currently less configurable than its model.
//! 3. **No container density knob.** `ThemeMetrics` covers controls;
//!    container padding is baked into each constructor.
//! 4. **`Palette` is a closed match.** Carbon token names aren't palette
//!    members, so they resolve to literal rgba and don't follow a
//!    palette swap. An `extra: BTreeMap<&'static str, Color>` consulted
//!    by `lookup` on a miss would fix it in a few lines.

#![warn(missing_docs)]

pub mod inspector;
pub mod theme;
pub mod tokens;
pub mod widgets;

pub use inspector::TraceInspector;
