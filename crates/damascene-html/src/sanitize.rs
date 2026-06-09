//! Hardcoded sanitization policy for the v1 transformer.
//!
//! These rules apply before any tag-to-widget mapping: blocked tags
//! cause the entire subtree to be skipped, blocked attributes are
//! filtered when reading element attrs, and blocked URLs cause the
//! containing `href` / `src` to be treated as absent.
//!
//! Embedders handling untrusted HTML should still layer a dedicated
//! sanitizer (e.g. `ammonia`) in front; this is the irreducible safety
//! floor, not a security boundary on its own.

/// Tags whose entire subtree the walker skips. Mix of script / plugin
/// hosts (security) and document-shell tags that don't render content
/// (`<head>`, `<meta>`, `<link>`, `<title>`).
pub(crate) fn is_blocked_tag(name: &str) -> bool {
    matches!(
        name,
        "script"
            | "iframe"
            | "object"
            | "embed"
            | "noscript"
            | "head"
            | "meta"
            | "link"
            | "title"
            | "base"
            | "template"
            | "frame"
            | "frameset"
            // `<style>` element bodies are collected at entry by
            // `collect_stylesheets` and applied through the cascade;
            // the element itself must not render its CSS text as a
            // paragraph during the regular walk.
            | "style"
    )
}

/// Attributes the walker silently drops. Event handlers and a couple of
/// historical scripting hooks. Style attributes pass through; the v1
/// transformer ignores their values but the v2 CSS pass will read
/// them.
pub(crate) fn is_blocked_attr(name: &str) -> bool {
    // Every `on*` attribute (onclick, onload, onerror, …).
    if name.starts_with("on") {
        return true;
    }
    matches!(name, "srcdoc" | "formaction")
}

/// Returns `true` if `url` is in the allowed scheme set: `http(s)`,
/// `mailto`, `tel`, `data:image/*`, or scheme-relative / path-relative.
/// Used to validate `href` and `src` values before they reach the El
/// tree.
pub(crate) fn is_safe_url(url: &str) -> bool {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Path-relative, root-relative, and fragment / query references
    // never carry a scheme — accept directly.
    if !contains_scheme(trimmed) {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("tel:")
    {
        return true;
    }
    if let Some(rest) = lower.strip_prefix("data:") {
        // Only raster image data URLs — drops `data:text/html` and
        // friends, which can carry script payloads, and SVG, which can
        // embed `<script>` when opened as a document.
        return rest.starts_with("image/") && !rest.starts_with("image/svg");
    }
    false
}

fn contains_scheme(url: &str) -> bool {
    // A URL has a scheme if some prefix matches `[a-zA-Z][a-zA-Z0-9+.-]*:`.
    // Cheap manual scan — avoids pulling a URL crate for this single
    // check.
    let bytes = url.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    for (i, &b) in bytes.iter().enumerate().skip(1) {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'+' | b'.' | b'-' => continue,
            b':' => return i > 0,
            _ => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_svg_urls_are_rejected() {
        assert!(is_safe_url("data:image/png;base64,AAAA"));
        assert!(!is_safe_url("data:image/svg+xml,<svg onload=alert(1)/>"));
        assert!(!is_safe_url("DATA:image/SVG+xml;base64,AAAA"));
    }

    #[test]
    fn allowed_urls_pass() {
        for url in [
            "https://damascene.dev",
            "http://example.com/path",
            "mailto:user@example.com",
            "tel:+15551234",
            "/relative/path",
            "./file.html",
            "../sibling",
            "#anchor",
            "?query=1",
            "data:image/png;base64,AAA",
        ] {
            assert!(is_safe_url(url), "expected safe: {url}");
        }
    }

    #[test]
    fn dangerous_urls_blocked() {
        for url in [
            "javascript:alert(1)",
            "JAVASCRIPT:alert(1)",
            "vbscript:msgbox",
            "data:text/html,<script>",
            "data:application/javascript,alert(1)",
            "",
            "   ",
        ] {
            assert!(!is_safe_url(url), "expected blocked: {url}");
        }
    }

    #[test]
    fn on_attrs_blocked() {
        assert!(is_blocked_attr("onclick"));
        assert!(is_blocked_attr("onerror"));
        assert!(is_blocked_attr("onmouseover"));
        assert!(!is_blocked_attr("href"));
        assert!(!is_blocked_attr("class"));
    }

    #[test]
    fn script_and_iframe_blocked() {
        assert!(is_blocked_tag("script"));
        assert!(is_blocked_tag("iframe"));
        assert!(is_blocked_tag("object"));
        assert!(!is_blocked_tag("div"));
        assert!(!is_blocked_tag("p"));
    }
}
