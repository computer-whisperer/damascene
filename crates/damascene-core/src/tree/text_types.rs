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
    /// A font family supplied by the app — the selection half of
    /// [`crate::text::registry::register_font`] (issue #136).
    /// Construct via [`FontFamily::custom`]; the payload is an
    /// interned id, not the name itself — every `El` carries two
    /// `FontFamily` fields, and embedding a `&'static str` would grow
    /// the node past its size budget.
    Custom(CustomFamilyId),
}

/// Interned identifier for a [`FontFamily::Custom`] family name.
/// Created by [`FontFamily::custom`]; resolve back with
/// [`Self::name`]. Ids are process-global: the same name always
/// interns to the same id, so derived `Eq`/`Hash` compare families
/// correctly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CustomFamilyId(u8);

/// Interned custom family names, indexed by [`CustomFamilyId`].
/// Append-only and tiny (one entry per distinct custom family the
/// app selects).
static CUSTOM_FAMILY_NAMES: std::sync::RwLock<Vec<&'static str>> =
    std::sync::RwLock::new(Vec::new());

impl CustomFamilyId {
    /// The family name this id was interned from.
    pub fn name(self) -> &'static str {
        CUSTOM_FAMILY_NAMES
            .read()
            .expect("custom family registry poisoned")[self.0 as usize]
    }
}

impl FontFamily {
    /// A font family supplied by the app, named as the registered face
    /// reports it (e.g. `"Work Sans"`). Shaping and the glyph atlas
    /// resolve the name against the font database, so a face supplied
    /// via [`crate::text::registry::register_font`] can be the
    /// *primary* family, not just a fallback (issue #136):
    ///
    /// ```ignore
    /// damascene_core::text::registry::register_font(WORK_SANS_BYTES.to_vec());
    /// let theme = Theme::damascene_dark()
    ///     .with_font_family(FontFamily::custom("Work Sans"));
    /// ```
    ///
    /// Register every weight the app uses (or one variable face); the
    /// shaper picks the family's closest face per weight. Matching is
    /// exact and case-sensitive against the face's typographic family
    /// name (name ID 16, else name ID 1). Glyphs the family lacks —
    /// or the whole run, when no face with this name was registered —
    /// resolve through the usual per-codepoint fallback; that keeps a
    /// typo rendering instead of failing, but the fallback face is
    /// whatever covers the codepoint, not the default UI font, so it
    /// is visibly wrong — check the name if text looks off.
    ///
    /// Names are interned process-wide; calling with the same name
    /// returns an equal value. At most 255 distinct custom family
    /// names may be interned (far beyond any real app's brand-font
    /// count) — exceeding that panics.
    pub fn custom(name: &'static str) -> Self {
        // Fast path: already interned — a read lock suffices, so
        // per-frame view code re-selecting the same family never
        // contends on the write lock.
        {
            let names = CUSTOM_FAMILY_NAMES
                .read()
                .expect("custom family registry poisoned");
            if let Some(index) = names.iter().position(|n| *n == name) {
                return FontFamily::Custom(CustomFamilyId(index as u8));
            }
        }
        let mut names = CUSTOM_FAMILY_NAMES
            .write()
            .expect("custom family registry poisoned");
        // Re-probe: another thread may have interned between the locks.
        let index = match names.iter().position(|n| *n == name) {
            Some(index) => index,
            None if names.len() >= u8::MAX as usize => {
                // Panic without holding the guard — poisoning the
                // registry would take down `family_name()` for every
                // already-valid id.
                drop(names);
                panic!("too many distinct custom font families");
            }
            None => {
                names.push(name);
                names.len() - 1
            }
        };
        FontFamily::Custom(CustomFamilyId(index as u8))
    }

    /// Canonical face name as registered in the font database (e.g.
    /// `"Inter Variable"`) — what text shaping and the glyph atlas look
    /// up.
    pub fn family_name(self) -> &'static str {
        match self {
            FontFamily::Inter => "Inter Variable",
            FontFamily::Roboto => "Roboto",
            FontFamily::JetBrainsMono => "JetBrains Mono",
            FontFamily::Custom(id) => id.name(),
        }
    }

    /// CSS `font-family` fallback stack for this face, for integrations
    /// that mirror Damascene typography into web / CSS contexts.
    /// [`FontFamily::Custom`] composes its name ahead of the sans-serif
    /// system stack (the only variant that allocates).
    pub fn css_stack(self) -> std::borrow::Cow<'static, str> {
        match self {
            FontFamily::Inter => {
                "'Inter Variable', Inter, ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, sans-serif".into()
            }
            FontFamily::Roboto => {
                "Roboto, ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif".into()
            }
            FontFamily::JetBrainsMono => {
                "'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Consolas, monospace".into()
            }
            FontFamily::Custom(id) => format!(
                "'{}', ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, sans-serif",
                // App-supplied name inside a CSS string: escape the
                // string delimiter (fonts named with apostrophes
                // exist) so the declaration stays parseable.
                id.name().replace('\'', "\\'")
            )
            .into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_families_intern_by_name() {
        let a = FontFamily::custom("Test Family A");
        let b = FontFamily::custom("Test Family B");
        assert_eq!(a, FontFamily::custom("Test Family A"));
        assert_ne!(a, b);
        assert_eq!(a.family_name(), "Test Family A");
        assert_eq!(b.family_name(), "Test Family B");
        assert_ne!(a, FontFamily::Inter);
    }

    #[test]
    fn custom_css_stack_composes_name_with_sans_fallbacks() {
        let stack = FontFamily::custom("Work Sans").css_stack();
        assert!(stack.starts_with("'Work Sans', "), "stack={stack}");
        assert!(stack.ends_with("sans-serif"), "stack={stack}");
    }
}
