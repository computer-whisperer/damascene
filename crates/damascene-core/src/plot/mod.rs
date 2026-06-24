//! Backend-neutral data and interaction state for the 2D `plot` widget:
//! polished, hardware-accelerated 2D plots and graphs (time series, scatter,
//! and — as the feature lands — area and bar charts).
//!
//! Scope and rationale live in `docs/PLOT2D_PLAN.md`. The short version: a
//! 2D plot's data marks are a degenerate 3D scene (orthographic, all
//! geometry at `z = 0`), so `plot` **reuses the shipped [`scene`](crate::scene)
//! GPU pipelines** — screen-pixel anti-aliased lines and points, offscreen
//! MSAA, color-managed linear blending, and the versioned-handle upload
//! cache — rather than adding a parallel 2D renderer. This module owns the
//! *data* and *interaction state* the widget carries; the pipelines that
//! render it live in the backend crates, never here.
//!
//! ## API layering
//!
//! The surface is layered so a simple plot is a one-liner while a power user
//! can build a custom plotting system from the same pieces:
//!
//! 1. `plot([...marks])` — the common case; auto-scaled axes, pan/zoom,
//!    and a crosshair for free.
//! 2. [`Scale`] + axis configuration — override the warp (linear/time/log),
//!    ticks, and formatting per axis.
//! 3. The public primitives — [`Scale`] and [`PlotView`] (here), plus the
//!    series handle and data op — compose a bespoke chart with no `plot()`
//!    at all.

pub mod scale;
pub mod view;

pub use scale::{Scale, Tick};
pub use view::{AxisView, PlotView};
