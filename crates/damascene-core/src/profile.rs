//! Span-tracing primitives gated behind the `profiling` Cargo feature.
//!
//! Off by default. When enabled, every [`crate::profile_span!`] call enters a
//! `tracing::trace_span!` whose guard drops on scope exit — so spans
//! nest exactly the way Rust scopes nest, which is what
//! `tracing-chrome` / perfetto need to draw a clean flame chart.
//!
//! When the feature is off there is no `tracing` dependency at all; the
//! macro expands to a `let _: () = ();` binding and disappears in any
//! optimization pass. Release builds without `--features profiling`
//! pay zero CPU and zero binary size for instrumented call sites.
//!
//! ## Usage
//!
//! ```ignore
//! use damascene_core::profile_span;
//!
//! fn layout_pass() {
//!     profile_span!("layout");
//!     // ... work ...
//! }
//! ```
//!
//! ## Host wiring
//!
//! Subscribers live in the host binary, not here. The showcase wires up
//! `tracing-chrome` behind a `--profile <output.json>` flag. Other apps
//! can attach `tracing-subscriber::fmt` for live console output, or any
//! other subscriber from the `tracing` ecosystem.
//!
//! ## Naming
//!
//! Span names are short stable strings using `phase::sub` shape so the
//! flame chart reads top-down. Examples: `frame::build`,
//! `prepare::layout`, `paint::text::shape`. Prefer adding a new span
//! name over reusing an existing one for a different call site —
//! flame-chart readers identify hotspots by name.

/// Enter a span for the rest of the current scope. No-op unless the
/// `profiling` feature is enabled. Pass a `&'static str` literal — the
/// macro forwards it to `tracing::trace_span!` (or to a `()` binding
/// when off).
///
/// ```ignore
/// fn layout(...) {
///     profile_span!("layout");
///     // ... work ...
/// }
/// ```
#[macro_export]
macro_rules! profile_span {
    ($name:expr $(,)?) => {
        let _damascene_profile_guard = $crate::profile::__enter($name);
    };
}

#[cfg(feature = "profiling")]
#[doc(hidden)]
#[inline]
pub fn __enter(name: &'static str) -> tracing::span::EnteredSpan {
    tracing::trace_span!(target: "damascene", "span", name = name).entered()
}

#[cfg(not(feature = "profiling"))]
#[doc(hidden)]
#[inline]
pub fn __enter(_name: &'static str) {}
