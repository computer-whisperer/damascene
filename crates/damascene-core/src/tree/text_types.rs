//! Text styling enums carried by [`El`](crate::El).

/// Font weight. The renderer maps these to font-loading or to
/// font-weight CSS / SVG attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FontWeight {
    #[default]
    Regular,
    Medium,
    Semibold,
    Bold,
}

/// A bundled or named font family selectable by the theme. The enum
/// covers the proportional UI faces (`Inter`, `Roboto`) and the
/// monospace face (`JetBrainsMono`); themes carry one slot for each
/// role (`Theme::font_family`, `Theme::mono_font_family`), and any
/// run can override per-node via `.font_family(...)` /
/// `.mono_font_family(...)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum FontFamily {
    /// Inter Variable, the closest bundled match for modern shadcn /
    /// Tailwind SaaS-dashboard typography.
    #[default]
    Inter,
    /// Roboto, retained for Material-style applications and backward
    /// compatibility with early Damascene typography.
    Roboto,
    /// JetBrains Mono Variable, the bundled monospace face used for
    /// code blocks, inline code, and any node tagged via `.mono()` or
    /// `TextRole::Code`. Default value of `Theme::mono_font_family`.
    JetBrainsMono,
}

impl FontFamily {
    pub fn family_name(self) -> &'static str {
        match self {
            FontFamily::Inter => "Inter Variable",
            FontFamily::Roboto => "Roboto",
            FontFamily::JetBrainsMono => "JetBrains Mono",
        }
    }

    pub fn css_stack(self) -> &'static str {
        match self {
            FontFamily::Inter => {
                "'Inter Variable', Inter, ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, sans-serif"
            }
            FontFamily::Roboto => {
                "Roboto, ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif"
            }
            FontFamily::JetBrainsMono => {
                "'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TextWrap {
    #[default]
    NoWrap,
    Wrap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TextOverflow {
    #[default]
    Clip,
    Ellipsis,
}

/// Semantic typography role for text-bearing nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TextRole {
    #[default]
    Body,
    Caption,
    Label,
    Title,
    Heading,
    Display,
    Code,
}

impl TextRole {
    pub fn name(self) -> &'static str {
        match self {
            TextRole::Body => "body",
            TextRole::Caption => "caption",
            TextRole::Label => "label",
            TextRole::Title => "title",
            TextRole::Heading => "heading",
            TextRole::Display => "display",
            TextRole::Code => "code",
        }
    }
}
