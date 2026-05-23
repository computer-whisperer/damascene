//! CSS selector parsing + cascade for tier-2B.
//!
//! Scope per `docs/HTML_VISION.md`:
//!
//! - Selector forms: tag (`p`, `*`), class (`.foo`), id (`#bar`), and
//!   compound (`p.note#main`), comma-grouped (`p, h1, .quote`).
//! - No descendant / child / sibling / pseudo combinators. Selectors
//!   containing whitespace, `>`, `+`, `~`, `:`, `[`, or `@` are
//!   rejected at parse time and silently dropped.
//! - Specificity: standard CSS `(id_count, class_count, tag_count)`.
//!   Compared tuple-wise; ties broken by source order.
//! - At-rules (`@media`, `@import`, `@font-face`, …) are skipped
//!   wholesale — the entire `{ ... }` block is consumed and dropped
//!   so nested rules inside it don't leak into the top-level rule
//!   list.
//!
//! The cascade applies in three layers, lowest priority first:
//!
//! 1. Matching `<style>` block rules, sorted by `(specificity,
//!    source_order)`.
//! 2. The element's inline `style="..."` declarations.
//! 3. Tag-default behaviour on the Aetna El (e.g. `<mark>`'s
//!    yellow background).
//!
//! Layer 1 is what this module produces. The transformer's existing
//! `cascade_style` helper layers 2 on top, and the tag dispatchers
//! finally apply 3 through the inline-state value-override fields.

use crate::css::{ComputedStyle, parse_inline_style};
use crate::lints::{FindingKind, Lints};

/// One parsed `<style>` rule.
#[derive(Debug)]
pub(crate) struct Rule {
    pub selectors: Vec<Selector>,
    pub declarations: ComputedStyle,
    /// Stable monotonic counter assigned at parse time so equal-
    /// specificity rules later in source order win.
    pub source_order: u32,
}

/// A flat collection of rules. Documents with no `<style>` blocks
/// produce an empty stylesheet; cascade lookups against it return a
/// default `ComputedStyle`.
#[derive(Debug, Default)]
pub(crate) struct Stylesheet {
    rules: Vec<Rule>,
}

impl Stylesheet {
    /// Compute the cascaded style for an element identified by its
    /// `tag`, `classes`, and optional `id`. Matching rules are
    /// collected, sorted by `(specificity, source_order)`, and
    /// flattened into a single `ComputedStyle` where later (winning)
    /// declarations overwrite earlier ones.
    pub(crate) fn cascade(&self, tag: &str, classes: &[&str], id: Option<&str>) -> ComputedStyle {
        let mut matches: Vec<(Specificity, u32, &ComputedStyle)> = Vec::new();
        for rule in &self.rules {
            // A rule with N selectors matches if any one of them
            // matches; record the winner's specificity. (Matching the
            // first qualifying selector is fine — the comma-grouped
            // form is purely shorthand for repeating the declaration
            // block.)
            let mut hit: Option<Specificity> = None;
            for sel in &rule.selectors {
                if sel.matches(tag, classes, id) {
                    let spec = sel.specificity();
                    hit = Some(match hit {
                        Some(prev) if prev > spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = hit {
                matches.push((spec, rule.source_order, &rule.declarations));
            }
        }
        matches.sort_by_key(|&(spec, order, _)| (spec, order));
        let mut out = ComputedStyle::default();
        for (_, _, decls) in matches {
            out.merge(decls);
        }
        out
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// A compound selector: optional tag (lowercased), zero-or-more
/// classes, optional id. `tag = None` represents `*` (universal).
#[derive(Debug, Clone)]
pub(crate) struct Selector {
    pub tag: Option<String>,
    pub classes: Vec<String>,
    pub id: Option<String>,
}

impl Selector {
    /// Standard CSS specificity ordering: id count > class count >
    /// tag count. Tag-only and `*` both rank `(0, 0, 0/1)`.
    pub(crate) fn specificity(&self) -> Specificity {
        Specificity {
            id: u32::from(self.id.is_some()),
            class: self.classes.len() as u32,
            tag: u32::from(self.tag.is_some()),
        }
    }

    /// Does this selector match an element with the given tag /
    /// classes / id? Tag comparison is ASCII case-insensitive (HTML
    /// rule); class and id are case-sensitive.
    pub(crate) fn matches(&self, tag: &str, classes: &[&str], id: Option<&str>) -> bool {
        if let Some(t) = &self.tag
            && !tag.eq_ignore_ascii_case(t)
        {
            return false;
        }
        if let Some(my_id) = &self.id
            && id != Some(my_id.as_str())
        {
            return false;
        }
        for c in &self.classes {
            if !classes.iter().any(|el_class| el_class == c) {
                return false;
            }
        }
        true
    }
}

/// Lexicographic specificity tuple. `PartialOrd`/`Ord` give us a
/// direct sort key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Specificity {
    pub id: u32,
    pub class: u32,
    pub tag: u32,
}

// ---------- Parsing ----------

/// Parse the content of a `<style>` block into rules.
pub(crate) fn parse_stylesheet(input: &str, lints: &Lints) -> Vec<Rule> {
    let cleaned = strip_comments(input);
    let bytes = cleaned.as_bytes();
    let mut out = Vec::new();
    let mut order: u32 = 0;
    let mut cursor = 0;

    while cursor < bytes.len() {
        // Skip leading whitespace.
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            break;
        }
        // Find the start of the next decl block.
        let Some(brace) = find_top_level(&cleaned[cursor..], b'{') else {
            break;
        };
        let abs_brace = cursor + brace;
        let prelude = cleaned[cursor..abs_brace].trim();
        let Some(close) = find_matching_brace(&cleaned, abs_brace) else {
            break;
        };
        let body = &cleaned[abs_brace + 1..close];

        if prelude.starts_with('@') {
            // At-rule — consume the block and move on. We don't
            // recurse into `@media` / `@supports` bodies in v1;
            // future work could lift rules out when the at-rule
            // unconditionally applies.
            cursor = close + 1;
            continue;
        }

        let selectors = parse_selector_list(prelude, lints);
        if !selectors.is_empty() {
            let declarations = parse_inline_style(body, lints);
            out.push(Rule {
                selectors,
                declarations,
                source_order: order,
            });
            order += 1;
        }
        cursor = close + 1;
    }
    out
}

/// Strip `/* ... */` comments from a CSS source. Unterminated
/// comments swallow everything to EOF (matches the CSS spec).
fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // Look for closing `*/`.
            let mut j = i + 2;
            while j + 1 < bytes.len() && !(bytes[j] == b'*' && bytes[j + 1] == b'/') {
                j += 1;
            }
            i = (j + 2).min(bytes.len());
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// Find the first `target` byte outside of nested parens / quotes /
/// braces. Returns an offset relative to `s`.
fn find_top_level(s: &str, target: u8) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut paren_depth: i32 = 0;
    let mut brace_depth: i32 = 0;
    let mut quote: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate() {
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'{' if target != b'{' => brace_depth += 1,
            b'}' if target != b'{' => brace_depth = brace_depth.saturating_sub(1),
            x if x == target && paren_depth == 0 && brace_depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Given the index of an opening `{`, find the matching closing `}`.
fn find_matching_brace(s: &str, open: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    debug_assert_eq!(bytes[open], b'{');
    let mut depth = 1;
    let mut quote: Option<u8> = None;
    for (offset, &b) in bytes[open + 1..].iter().enumerate() {
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + 1 + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_selector_list(input: &str, lints: &Lints) -> Vec<Selector> {
    let mut out = Vec::new();
    for raw in input.split(',') {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        match parse_compound_selector(trimmed) {
            Some(sel) => out.push(sel),
            None => lints.push(
                FindingKind::UnsupportedSelector,
                format!("`{trimmed}` (only tag / class / id / compound selectors are supported)"),
            ),
        }
    }
    out
}

/// Parse one compound selector: optional leading tag name (or `*`),
/// followed by zero-or-more `.class` / `#id` segments. Returns
/// `None` for anything else, including selectors with combinators
/// (` `, `>`, `+`, `~`), pseudo-classes (`:hover`), attribute
/// selectors (`[name=value]`), or namespace prefixes (`@`).
fn parse_compound_selector(input: &str) -> Option<Selector> {
    if input.is_empty() {
        return None;
    }
    // Reject anything we don't support outright.
    for c in input.chars() {
        if matches!(c, ' ' | '\t' | '\n' | '>' | '+' | '~' | ':' | '[' | '@') {
            return None;
        }
    }
    if input == "*" {
        return Some(Selector {
            tag: None,
            classes: Vec::new(),
            id: None,
        });
    }
    let bytes = input.as_bytes();
    let mut sel = Selector {
        tag: None,
        classes: Vec::new(),
        id: None,
    };
    let mut cursor = 0;

    // Optional tag at start.
    if !matches!(bytes[0], b'.' | b'#') {
        let end = bytes
            .iter()
            .position(|b| matches!(*b, b'.' | b'#'))
            .unwrap_or(bytes.len());
        let tag = input[..end].to_ascii_lowercase();
        if !is_valid_ident(&tag) {
            return None;
        }
        sel.tag = Some(tag);
        cursor = end;
    }

    while cursor < bytes.len() {
        let sigil = bytes[cursor];
        let after = cursor + 1;
        let end = bytes[after..]
            .iter()
            .position(|b| matches!(*b, b'.' | b'#'))
            .map(|p| p + after)
            .unwrap_or(bytes.len());
        let name = &input[after..end];
        if !is_valid_ident(name) {
            return None;
        }
        match sigil {
            b'.' => sel.classes.push(name.to_string()),
            b'#' => {
                if sel.id.is_some() {
                    return None;
                }
                sel.id = Some(name.to_string());
            }
            _ => return None,
        }
        cursor = end;
    }
    Some(sel)
}

fn is_valid_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl Stylesheet {
    /// Build a stylesheet from one or more `<style>` block bodies,
    /// preserving source order across blocks. Findings from each
    /// block's parse (unsupported selector forms, dropped CSS
    /// declarations, unsupported units) accumulate in `lints`.
    pub(crate) fn from_blocks<'a>(
        blocks: impl IntoIterator<Item = &'a str>,
        lints: &Lints,
    ) -> Self {
        let mut rules = Vec::new();
        let mut order: u32 = 0;
        for block in blocks {
            for mut rule in parse_stylesheet(block, lints) {
                // Re-stamp source order so multiple blocks compose
                // sensibly. Within one block, parse_stylesheet
                // already starts at 0; we rewrite to the global
                // counter to preserve "later block beats earlier
                // block" semantics.
                rule.source_order = order;
                order += 1;
                rules.push(rule);
            }
        }
        Self { rules }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aetna_core::prelude::*;

    #[test]
    fn parses_tag_class_id_selectors() {
        let s = parse_compound_selector("p").unwrap();
        assert_eq!(s.tag.as_deref(), Some("p"));
        assert!(s.classes.is_empty());
        assert!(s.id.is_none());

        let s = parse_compound_selector(".note").unwrap();
        assert!(s.tag.is_none());
        assert_eq!(s.classes, vec!["note"]);

        let s = parse_compound_selector("#main").unwrap();
        assert_eq!(s.id.as_deref(), Some("main"));

        let s = parse_compound_selector("p.note#main").unwrap();
        assert_eq!(s.tag.as_deref(), Some("p"));
        assert_eq!(s.classes, vec!["note"]);
        assert_eq!(s.id.as_deref(), Some("main"));

        let s = parse_compound_selector("*").unwrap();
        assert!(s.tag.is_none());
        assert!(s.classes.is_empty());
        assert!(s.id.is_none());
    }

    #[test]
    fn rejects_unsupported_selectors() {
        assert!(parse_compound_selector("p span").is_none());
        assert!(parse_compound_selector("p > span").is_none());
        assert!(parse_compound_selector("a:hover").is_none());
        assert!(parse_compound_selector("input[type]").is_none());
        assert!(parse_compound_selector("@media").is_none());
        assert!(parse_compound_selector(".foo+.bar").is_none());
        assert!(parse_compound_selector("").is_none());
    }

    #[test]
    fn parses_comma_grouped_selectors() {
        let lints = Lints::default();
        let list = parse_selector_list("p, h1, .note", &lints);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].tag.as_deref(), Some("p"));
        assert_eq!(list[1].tag.as_deref(), Some("h1"));
        assert_eq!(list[2].classes, vec!["note"]);
        assert!(lints.into_vec().is_empty());
    }

    #[test]
    fn unsupported_selectors_lint_individually() {
        let lints = Lints::default();
        let list = parse_selector_list("p > span, .note, a:hover", &lints);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].classes, vec!["note"]);
        let findings = lints.into_vec();
        assert_eq!(findings.len(), 2);
        assert!(
            findings
                .iter()
                .all(|f| matches!(f.kind, crate::lints::FindingKind::UnsupportedSelector))
        );
    }

    #[test]
    fn specificity_ordering() {
        let tag = parse_compound_selector("p").unwrap().specificity();
        let class = parse_compound_selector(".foo").unwrap().specificity();
        let id = parse_compound_selector("#main").unwrap().specificity();
        let compound = parse_compound_selector("p.foo#main").unwrap().specificity();
        assert!(class > tag);
        assert!(id > class);
        assert!(compound > id);
        assert!(compound > class);
    }

    #[test]
    fn selector_matches_use_case_rules() {
        let s = parse_compound_selector("P").unwrap();
        // Tag is normalised to lowercase, then matched case-insensitively.
        assert!(s.matches("p", &[], None));
        assert!(s.matches("P", &[], None));

        let c = parse_compound_selector(".Note").unwrap();
        // Class names are case-sensitive.
        assert!(c.matches("div", &["Note"], None));
        assert!(!c.matches("div", &["note"], None));

        let id = parse_compound_selector("#main").unwrap();
        assert!(id.matches("div", &[], Some("main")));
        assert!(!id.matches("div", &[], Some("Main")));
    }

    #[test]
    fn stylesheet_skips_at_rules() {
        let lints = Lints::default();
        let rules = parse_stylesheet(
            "@media print { p { color: red } } h1 { color: blue }",
            &lints,
        );
        // The h1 rule should be present, the @media block dropped wholesale.
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selectors[0].tag.as_deref(), Some("h1"));
    }

    #[test]
    fn stylesheet_strips_comments() {
        let lints = Lints::default();
        let rules = parse_stylesheet("/* skip me */ p { color: red /* inline */ }", &lints);
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules[0].declarations.text_color,
            Some(Color::srgb_u8(255, 0, 0))
        );
    }

    #[test]
    fn cascade_picks_higher_specificity_then_source_order() {
        let lints = Lints::default();
        let sheet = Stylesheet::from_blocks(
            ["p { color: red } p.note { color: blue } p { color: green }"],
            &lints,
        );
        let s = sheet.cascade("p", &["note"], None);
        assert_eq!(s.text_color, Some(Color::srgb_u8(0, 0, 255)));

        let s = sheet.cascade("p", &[], None);
        assert_eq!(s.text_color, Some(Color::srgb_u8(0, 128, 0)));
    }

    #[test]
    fn cascade_merges_different_props_across_rules() {
        let lints = Lints::default();
        let sheet = Stylesheet::from_blocks(["p { color: red } p { font-weight: bold }"], &lints);
        let s = sheet.cascade("p", &[], None);
        assert_eq!(s.text_color, Some(Color::srgb_u8(255, 0, 0)));
        assert_eq!(s.font_weight, Some(FontWeight::Bold));
    }

    #[test]
    fn cascade_id_beats_compound_class() {
        let lints = Lints::default();
        let sheet =
            Stylesheet::from_blocks(["#main { color: blue } .a.b.c.d { color: red }"], &lints);
        let s = sheet.cascade("div", &["a", "b", "c", "d"], Some("main"));
        assert_eq!(s.text_color, Some(Color::srgb_u8(0, 0, 255)));
    }
}
