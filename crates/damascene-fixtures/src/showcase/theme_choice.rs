//! User-selectable theme — drives the showcase's `App::theme` impl so the
//! showcase renders against the chosen palette without rebuilding state.
//!
//! Eight themes total: Damascene's own dark/light pair (= shadcn zinc, the
//! palette Damascene ships against by default) plus three Radix Colors
//! pairs — slate + blue, sand + amber, and mauve + violet. The picker
//! in the sidebar swaps between these live; every page reflects the
//! choice on the next frame because token lookups resolve through the
//! active palette at paint time.

use damascene_core::prelude::Theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ThemeChoice {
    #[default]
    DamasceneDark,
    DamasceneLight,
    RadixSlateBlueDark,
    RadixSlateBlueLight,
    RadixSandAmberDark,
    RadixSandAmberLight,
    RadixMauveVioletDark,
    RadixMauveVioletLight,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 8] = [
        ThemeChoice::DamasceneDark,
        ThemeChoice::DamasceneLight,
        ThemeChoice::RadixSlateBlueDark,
        ThemeChoice::RadixSlateBlueLight,
        ThemeChoice::RadixSandAmberDark,
        ThemeChoice::RadixSandAmberLight,
        ThemeChoice::RadixMauveVioletDark,
        ThemeChoice::RadixMauveVioletLight,
    ];

    /// Stable identifier used as the value in routed select keys
    /// (`theme-picker:option:<token>`) and serialized state.
    pub fn token(self) -> &'static str {
        match self {
            ThemeChoice::DamasceneDark => "damascene-dark",
            ThemeChoice::DamasceneLight => "damascene-light",
            ThemeChoice::RadixSlateBlueDark => "radix-slate-blue-dark",
            ThemeChoice::RadixSlateBlueLight => "radix-slate-blue-light",
            ThemeChoice::RadixSandAmberDark => "radix-sand-amber-dark",
            ThemeChoice::RadixSandAmberLight => "radix-sand-amber-light",
            ThemeChoice::RadixMauveVioletDark => "radix-mauve-violet-dark",
            ThemeChoice::RadixMauveVioletLight => "radix-mauve-violet-light",
        }
    }

    /// Human-readable label for the dropdown trigger / menu items.
    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::DamasceneDark => "Damascene · dark",
            ThemeChoice::DamasceneLight => "Damascene · light",
            ThemeChoice::RadixSlateBlueDark => "Radix slate + blue · dark",
            ThemeChoice::RadixSlateBlueLight => "Radix slate + blue · light",
            ThemeChoice::RadixSandAmberDark => "Radix sand + amber · dark",
            ThemeChoice::RadixSandAmberLight => "Radix sand + amber · light",
            ThemeChoice::RadixMauveVioletDark => "Radix mauve + violet · dark",
            ThemeChoice::RadixMauveVioletLight => "Radix mauve + violet · light",
        }
    }

    pub fn from_token(s: &str) -> Option<ThemeChoice> {
        ThemeChoice::ALL.iter().copied().find(|c| c.token() == s)
    }

    pub fn theme(self) -> Theme {
        match self {
            ThemeChoice::DamasceneDark => Theme::damascene_dark(),
            ThemeChoice::DamasceneLight => Theme::damascene_light(),
            ThemeChoice::RadixSlateBlueDark => Theme::radix_slate_blue_dark(),
            ThemeChoice::RadixSlateBlueLight => Theme::radix_slate_blue_light(),
            ThemeChoice::RadixSandAmberDark => Theme::radix_sand_amber_dark(),
            ThemeChoice::RadixSandAmberLight => Theme::radix_sand_amber_light(),
            ThemeChoice::RadixMauveVioletDark => Theme::radix_mauve_violet_dark(),
            ThemeChoice::RadixMauveVioletLight => Theme::radix_mauve_violet_light(),
        }
    }
}
