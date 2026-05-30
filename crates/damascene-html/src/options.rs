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
    /// blocks (which the v2 CSS pass would otherwise consume) and any
    /// inline `style=""` attributes. Default `false`; the v1 tier-1
    /// transformer ignores `<style>` either way, so this flag is a
    /// no-op until the CSS subset lands.
    pub sanitize_styles: bool,
}

impl HtmlOptions {
    pub fn sanitize_styles(mut self, enabled: bool) -> Self {
        self.sanitize_styles = enabled;
        self
    }
}
