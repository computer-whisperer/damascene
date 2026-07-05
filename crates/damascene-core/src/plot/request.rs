//! Programmatic [`plot()`](crate::tree::plot) view commands — the plot
//! counterpart of [`crate::viewport::ViewportRequest`].
//!
//! Apps accumulate requests from event handlers (a "Fit" toolbar button,
//! a "last 60 s" preset) and hand them to the host once per frame via
//! [`crate::event::App::drain_plot_requests`]; each is consumed during
//! [`UiState::prepare_plots`](crate::state::UiState) by the plot whose
//! `.key(...)` it names, where the live data bounds are known. This is
//! the app-side write half of the view API —
//! [`plot_view_by_key`](crate::state::UiState::plot_view_by_key) is the
//! read half — so `build` / `on_event` code can drive the view without
//! `&mut UiState`.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

/// What an app produces to drive a [`plot()`](crate::tree::plot)'s view
/// programmatically. Push once per build via
/// [`App::drain_plot_requests`](crate::event::App::drain_plot_requests);
/// unmatched requests (the keyed plot wasn't in the tree this frame) are
/// dropped, like [`crate::viewport::ViewportRequest`]s.
#[derive(Clone, Debug, PartialEq)]
pub enum PlotRequest {
    /// Re-fit both axes to the full data extent and restore per-axis
    /// autoscaling — the programmatic double-click reset. Re-arms
    /// [`x_autoscale`](crate::plot::PlotSpec::x_autoscale) /
    /// [`y_autoscale`](crate::plot::PlotSpec::y_autoscale) tracking that a
    /// manual gesture had released.
    FitAll {
        /// `.key(...)` of the target plot.
        key: String,
    },
    /// Pin the horizontal window to `[min, max]` in data space, taking
    /// manual X control (autoscale-X stops tracking until a reset /
    /// [`Self::FitAll`]). Y still autoscales to the new window unless the
    /// user holds manual Y. Ignored when the window is empty or
    /// non-finite.
    SetXWindow {
        /// `.key(...)` of the target plot.
        key: String,
        /// Lower edge of the window, in X data space.
        min: f64,
        /// Upper edge of the window, in X data space (must exceed `min`).
        max: f64,
    },
}

impl PlotRequest {
    /// The `.key(...)` of the plot this request targets.
    pub fn key(&self) -> &str {
        match self {
            PlotRequest::FitAll { key } | PlotRequest::SetXWindow { key, .. } => key,
        }
    }
}
