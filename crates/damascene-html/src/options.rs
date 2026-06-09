//! Output-changing options for the HTML transformer.

/// Optional knobs for [`crate::html_with_options`].
///
/// The default is "trusted-scrap" — no extra sanitization beyond the
/// hardcoded baseline that always strips `<script>`, `<iframe>`,
/// `<object>`, `<embed>`, `<noscript>`, every `on*` attribute, and
/// every `javascript:` / `vbscript:` / `data:text/html` URL.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HtmlOptions {
    /// When `true`, treat input as untrusted: also drop `<style>`
    /// blocks and any inline `style=""` attributes, unparsed. Each
    /// drop is recorded as a [`crate::FindingKind::SanitizedStyle`]
    /// finding. Default `false`.
    pub sanitize_styles: bool,
}

impl HtmlOptions {
    pub fn sanitize_styles(mut self, enabled: bool) -> Self {
        self.sanitize_styles = enabled;
        self
    }
}
