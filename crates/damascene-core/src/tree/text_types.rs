//! Text styling enums carried by [`El`](crate::El).

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

/// Font weight. The renderer maps these to font-loading or to
/// font-weight CSS / SVG attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FontWeight {
    /// Normal weight (CSS `400`). The default.
    #[default]
    Regular,
    /// Medium weight (CSS `500`). Used by labels and button text.
    Medium,
    /// Semibold weight (CSS `600`). Used by the title / heading roles.
    Semibold,
    /// Bold weight (CSS `700`). Used by the display role.
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
    /// Canonical face name as registered in the font database (e.g.
    /// `"Inter Variable"`) — what text shaping and the glyph atlas look
    /// up.
    pub fn family_name(self) -> &'static str {
        match self {
            FontFamily::Inter => "Inter Variable",
            FontFamily::Roboto => "Roboto",
            FontFamily::JetBrainsMono => "JetBrains Mono",
        }
    }

    /// CSS `font-family` fallback stack for this face, for integrations
    /// that mirror Damascene typography into web / CSS contexts.
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

/// Horizontal alignment of a text run within its resolved rect
/// (CSS `text-align`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextAlign {
    /// Align to the leading edge (left, in left-to-right text). The default.
    #[default]
    Start,
    /// Center within the rect.
    Center,
    /// Align to the trailing edge (right, in left-to-right text).
    End,
}

/// Line-wrapping policy for a text run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TextWrap {
    /// Single line; overflow is handled per
    /// [`TextOverflow`]. The default.
    #[default]
    NoWrap,
    /// Break into multiple lines at the available width (CSS
    /// `white-space: normal`). Opt in via `.wrap_text()`.
    Wrap,
}

/// What happens to text that exceeds the available rect
/// (CSS `text-overflow`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TextOverflow {
    /// Hard-clip overflowing glyphs at the rect edge. The default.
    #[default]
    Clip,
    /// Truncate with a `…` ellipsis. Opt in via `.ellipsis()`.
    Ellipsis,
}

/// Semantic typography role for text-bearing nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TextRole {
    /// Default body copy: `TEXT_SM` (14px), regular weight, foreground
    /// color.
    #[default]
    Body,
    /// Smallest helper text: `TEXT_XS` (12px), regular weight, muted
    /// foreground color. HTML analogue: `<small>` / help text.
    Caption,
    /// Form-label text: `TEXT_SM` (14px) at medium weight. HTML
    /// analogue: `<label>`.
    Label,
    /// Card / section title: `TEXT_BASE` (16px), semibold.
    Title,
    /// Page-section heading: `TEXT_2XL`, semibold. HTML analogue:
    /// `<h2>`-ish.
    Heading,
    /// Largest hero text: `TEXT_3XL`, bold. HTML analogue: `<h1>` /
    /// display type.
    Display,
    /// Code text: `TEXT_XS` (12px), regular weight, and **forces the
    /// monospace family** (`font_mono = true`) regardless of other
    /// modifiers.
    Code,
}

impl TextRole {
    /// Lowercase role name (`"body"`, `"caption"`, …) used by the
    /// bundle inspect dump and other diagnostics.
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
