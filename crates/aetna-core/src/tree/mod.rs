//! The [`El`] tree — the central data structure.
//!
//! An `El` is an HTML-DOM-shaped node: it has a [`Kind`] (semantic role),
//! styling, layout properties, optional text content, and zero or more
//! child `El`s. Build trees with the component constructors (`text`,
//! `button`, `card`, …) and the layout primitives (`column`, `row`,
//! `spacer`, `divider`).
//!
//! # Tree shape
//!
//! - Visual properties (`fill`, `stroke`, `radius`, `shadow`) live on
//!   `El` for the user-facing modifier API; at render time they resolve
//!   into [`crate::ir::DrawOp`]s bound to a stock shader
//!   ([`crate::shader::StockShader::RoundedRect`] for surfaces,
//!   [`crate::shader::StockShader::Text`] for text).
//! - [`El::shader_override`] lets a custom component bind its own shader
//!   instead of `rounded_rect` for the surface paint. The escape hatch
//!   the substrate must support — see `docs/SHADER_VISION.md`.
//!
//! # Source mapping for free
//!
//! Every constructor in this crate is `#[track_caller]`, so the call site
//! is captured automatically — no `src_here!` macro at every call. The
//! source location lives in [`El::source`] and flows through to the tree
//! dump and lint artifacts the agent loop consumes.

mod color;
mod constructors;
mod content;
mod defaults;
mod geometry;
mod icon_name;
mod identity;
mod layout_modifiers;
mod layout_types;
mod node;
mod semantics;
mod text_types;
mod visual_modifiers;

pub use color::Color;
pub use constructors::{
    column, divider, hard_break, image, math, math_block, math_inline, row, scroll, spacer, stack,
    surface, text_runs, vector, virtual_list, virtual_list_dyn,
};
pub use geometry::{Corners, Rect, Sides};
pub use icon_name::IconName;
pub use identity::HoverAlpha;
pub use layout_types::{Align, Axis, Justify, PinPolicy, Size};
pub use node::{El, FocusRingPlacement};
pub use semantics::{InteractionState, Kind, Source, SurfaceRole};
pub use text_types::{FontFamily, FontWeight, TextAlign, TextOverflow, TextRole, TextWrap};
