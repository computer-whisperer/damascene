//! Accessibility surfaces shared by every host arrangement.
//!
//! This module currently carries the *user-preference* half of the
//! accessibility story: [`AccessibilityPreferences`], the CSS
//! `prefers-*` media-feature family as a value the host pushes into the
//! runtime. The semantic half (roles, accessible names, the AccessKit
//! tree) lands in a later arc — see `docs/ACCESSIBILITY_PLAN.md`.
//!
//! Direction of flow: the **host** detects platform/user settings
//! (media queries on web, desktop portal / OS settings natively, env
//! overrides for testing) and calls the runner's
//! `set_accessibility_preferences`. The runtime honors what it owns
//! (motion policy — see below), and **apps** read the rest during build
//! via [`BuildCx::accessibility`] to make theme-shaped decisions
//! (palette choice for [`ColorScheme`], contrast variants). This is the
//! reverse of [`ColorPreferences`], where the *app* declares wishes and
//! the host negotiates — don't conflate the two.
//!
//! What the runtime honors automatically when
//! [`AccessibilityPreferences::reduced_motion`] is `Some(true)`:
//!
//! - App-driven movement props (`scale`, `translate`) snap to their
//!   targets instead of easing — enter transitions keep their opacity
//!   fade but lose zoom/slide, the toast exit keeps its fade but loses
//!   the sink, tap-bounces disappear.
//! - Viewport smooth navigation ([`ViewportRequest`] flights) becomes
//!   instant.
//! - Scene3D camera retargets (refocus / re-fit glides) snap to pose.
//!
//! Color and opacity easing (hover/press mixes, focus-ring fade, plain
//! fades), the caret blink, spinners, and time-driven shader uniforms
//! stay live: reduced motion targets *movement* — the
//! vestibular-trigger class — not essential feedback. This is a
//! deliberately different lever from
//! [`AnimationMode::Settled`](crate::state::AnimationMode), which
//! freezes everything for headless determinism.
//!
//! [`BuildCx::accessibility`]: crate::event::BuildCx::accessibility
//! [`ColorPreferences`]: crate::color::ColorPreferences
//! [`ViewportRequest`]: crate::viewport::ViewportRequest

/// User accessibility preferences reported by the host — the CSS
/// `prefers-*` media-feature family as a value.
///
/// Every field is an `Option`: `None` means the host doesn't know (or
/// the platform reports no preference), which is also the [`Default`].
/// Hosts construct via `Default` and set what they can detect:
///
/// ```
/// use damascene_core::a11y::{AccessibilityPreferences, ColorScheme};
///
/// let prefs = AccessibilityPreferences {
///     reduced_motion: Some(true),
///     color_scheme: Some(ColorScheme::Dark),
///     ..Default::default()
/// };
/// ```
///
/// Push with the runner's `set_accessibility_preferences` whenever a
/// value changes (hosts re-push the whole struct; there is no per-field
/// delta). Apps read the current value with
/// [`BuildCx::accessibility`](crate::event::BuildCx::accessibility) /
/// [`EventCx::accessibility`](crate::event::EventCx::accessibility).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AccessibilityPreferences {
    /// `prefers-reduced-motion` — `Some(true)` when the user asked the
    /// platform to minimize non-essential motion. The runtime honors
    /// this for library-owned movement automatically (see the module
    /// docs); apps additionally gate their own decorative motion on
    /// [`BuildCx::reduced_motion`](crate::event::BuildCx::reduced_motion).
    pub reduced_motion: Option<bool>,
    /// `prefers-color-scheme` — the user's light/dark preference. The
    /// runtime does not act on this (the app owns its [`Theme`]); read
    /// it during build to pick a palette.
    ///
    /// [`Theme`]: crate::Theme
    pub color_scheme: Option<ColorScheme>,
    /// `prefers-contrast` — the user asked for more or less contrast
    /// than the default. The runtime does not act on this; read it
    /// during build to pick a higher-contrast palette or strengthen
    /// borders. (Forced-colors mode is a future, separate field — it
    /// carries a whole system palette, not a direction.)
    pub contrast: Option<Contrast>,
    /// `prefers-reduced-transparency` — the user asked to minimize
    /// translucent surfaces. The runtime does not act on this; apps
    /// with translucent chrome (scrims, glassy panels) read it and
    /// opaque up.
    pub reduced_transparency: Option<bool>,
}

impl AccessibilityPreferences {
    /// `true` iff the user explicitly prefers reduced motion.
    /// (`None` — unknown — reads as `false`, matching the web's
    /// `no-preference` default.)
    pub fn prefers_reduced_motion(&self) -> bool {
        self.reduced_motion == Some(true)
    }
}

/// The user's `prefers-color-scheme` value: which of light or dark the
/// platform is set to. No `NoPreference` variant — that state is the
/// enclosing `Option` being `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorScheme {
    /// The platform is set to a light appearance.
    Light,
    /// The platform is set to a dark appearance.
    Dark,
}

/// The user's `prefers-contrast` direction. Mirrors the CSS values
/// `more` / `less`; the no-preference state is the enclosing `Option`
/// being `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contrast {
    /// The user asked for higher contrast (e.g. macOS "Increase
    /// contrast", GNOME high-contrast, `prefers-contrast: more`).
    More,
    /// The user asked for lower contrast (`prefers-contrast: less`).
    Less,
}
