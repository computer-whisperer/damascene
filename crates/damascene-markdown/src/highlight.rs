//! Syntect-driven syntax highlighting for fenced code blocks.
//!
//! The transformer hands `(language, source)` here when a fenced block
//! carries an info string syntect recognises; we walk the source through
//! [`syntect::parsing::ParseState`], maintain a [`ScopeStack`], and emit
//! one styled inline `El` per scope-homogeneous chunk. Newlines emit
//! [`hard_break`] runs so the result drops straight into a
//! [`text_runs`] paragraph.
//!
//! We deliberately bypass syntect's `Theme` / `Style` machinery. Themes
//! resolve scopes to concrete `syntect::highlighting::Color` values at
//! highlight time, but Damascene runs need [`Color::token`] references so
//! the palette swap (`Theme::damascene_dark()` ↔ `Theme::damascene_light()`)
//! re-tints the syntax run at paint time the same way it re-tints
//! every other surface. So we read the live `ScopeStack` at each
//! chunk and pick an Damascene palette token directly via
//! [`scope_to_damascene_color`].

use std::sync::OnceLock;

use damascene_core::tokens;
use damascene_core::tree::*;
use damascene_core::widgets::text::text;
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

/// Lazy-loaded merged [`SyntaxSet`]: syntect's bundled grammars plus
/// the `two-face` extras (full Rust grammar, TOML, GraphQL,
/// Dockerfile, etc.) in newlines mode so [`ParseState::parse_line`]
/// works. Loading deserialises a binary dump, so we pay it once per
/// process.
fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

/// Look up a syntax by its info-string label (`rust`, `py`, `python`,
/// `ts`, …). Returns `None` for unknown / empty labels — callers fall
/// back to the plain-mono code-block path.
pub(crate) fn find_syntax(label: &str) -> Option<&'static SyntaxReference> {
    let label = label.trim();
    if label.is_empty() {
        return None;
    }
    let set = syntax_set();
    set.find_syntax_by_token(label)
        .or_else(|| set.find_syntax_by_extension(label))
        .or_else(|| set.find_syntax_by_name(label))
}

/// Highlight `source` with the given syntax and return one styled inline
/// run per scope-homogeneous chunk. Newlines become [`hard_break`]
/// runs so the caller can wrap the result in [`text_runs`] and get a
/// faithful multi-line paragraph.
pub(crate) fn highlight_to_runs(source: &str, syntax: &SyntaxReference) -> Vec<El> {
    let set = syntax_set();
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut runs: Vec<El> = Vec::new();

    // `parse_line` requires a `\n`-terminated line. We split by `\n`
    // and emit a HardBreak between lines so the paragraph layouts
    // identically to the verbatim path. Any trailing (no final `\n`)
    // segment is parsed as a final line on its own.
    let mut lines = source.split('\n').peekable();
    while let Some(line) = lines.next() {
        let with_newline = format!("{line}\n");
        let ops = state.parse_line(&with_newline, set).unwrap_or_default();

        let mut last = 0usize;
        for (offset, op) in ops {
            // Emit the chunk that ran under the *current* stack before
            // applying the next op. `offset` indexes into `with_newline`.
            if offset > last && offset <= line.len() {
                let chunk = &line[last..offset];
                push_chunk(&mut runs, chunk, &stack);
            }
            stack.apply(&op).ok();
            last = offset;
        }
        // Tail of the line (everything past the last op, but excluding
        // the appended `\n` we added for the parser).
        if last < line.len() {
            push_chunk(&mut runs, &line[last..], &stack);
        }

        if lines.peek().is_some() {
            runs.push(hard_break());
        }
    }

    runs
}

fn push_chunk(out: &mut Vec<El>, chunk: &str, stack: &ScopeStack) {
    if chunk.is_empty() {
        return;
    }
    let color = scope_to_damascene_color(stack);
    out.push(text(chunk).mono().color(color));
}

/// Map the most-specific scope on the stack to an Damascene palette token.
/// We walk top-down (most specific first) and pick the first match,
/// falling back to [`tokens::FOREGROUND`] for unrecognised scopes.
///
/// The mapping is intentionally small — six categories cover the
/// majority of TextMate scope vocabulary across syntect's grammar set.
/// Adding a finer split (operators, types-vs-functions, builtins) is
/// a tuning pass for later, not a Phase 1 concern.
fn scope_to_damascene_color(stack: &ScopeStack) -> Color {
    for scope in stack.as_slice().iter().rev() {
        if let Some(color) = scope_color(*scope) {
            return color;
        }
    }
    tokens::FOREGROUND
}

fn scope_color(scope: Scope) -> Option<Color> {
    // syntect interns scopes; the human-readable form via Display is
    // dotted (e.g. "keyword.control.rust"). We compare prefixes so any
    // grammar-specific suffix still matches the broader category.
    let name = scope.build_string();
    if name.starts_with("comment") {
        return Some(tokens::MUTED_FOREGROUND);
    }
    if name.starts_with("string") {
        return Some(tokens::SUCCESS);
    }
    if name.starts_with("constant.numeric") || name.starts_with("constant.language") {
        return Some(tokens::INFO);
    }
    if name.starts_with("keyword") || name.starts_with("storage") {
        return Some(tokens::INFO);
    }
    if name.starts_with("entity.name.function")
        || name.starts_with("support.function")
        || name.starts_with("meta.function-call")
    {
        return Some(tokens::WARNING);
    }
    if name.starts_with("entity.name.type")
        || name.starts_with("support.type")
        || name.starts_with("support.class")
    {
        return Some(tokens::WARNING);
    }
    if name.starts_with("variable.parameter") {
        return Some(tokens::ACCENT_FOREGROUND);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_language_returns_none() {
        assert!(find_syntax("zalgo-text").is_none());
        assert!(find_syntax("").is_none());
    }

    #[test]
    fn known_languages_resolve() {
        assert!(find_syntax("rust").is_some());
        assert!(find_syntax("rs").is_some());
        assert!(find_syntax("python").is_some());
        assert!(find_syntax("toml").is_some());
    }

    #[test]
    fn rust_keyword_maps_to_keyword_color() {
        let syntax = find_syntax("rust").expect("rust syntax bundled");
        let runs = highlight_to_runs("fn main() {}\n", syntax);

        // We expect at least one Text run with the keyword color. The
        // exact run count depends on syntect's tokenisation, so we
        // check colors by collecting them.
        let colors: Vec<_> = runs
            .iter()
            .filter(|r| r.kind == Kind::Text)
            .filter_map(|r| r.text_color)
            .collect();
        assert!(
            colors.contains(&tokens::INFO),
            "expected `fn` keyword to map to INFO color, got colors: {colors:?}"
        );
        // All emitted text runs should ride on the mono path so the
        // theme's mono_font_family resolves at shape time.
        assert!(
            runs.iter()
                .filter(|r| r.kind == Kind::Text)
                .all(|r| r.font_mono),
            "every highlighted token should be font_mono = true"
        );
    }

    #[test]
    fn newlines_become_hard_breaks() {
        let syntax = find_syntax("rust").unwrap();
        let runs = highlight_to_runs("let a = 1;\nlet b = 2;\n", syntax);
        let breaks = runs.iter().filter(|r| r.kind == Kind::HardBreak).count();
        assert!(breaks >= 1, "expected at least one hard break for \\n");
    }

    #[test]
    fn comment_runs_use_muted_foreground() {
        let syntax = find_syntax("rust").unwrap();
        let runs = highlight_to_runs("// hello\nlet x = 1;\n", syntax);
        let colors: Vec<_> = runs
            .iter()
            .filter(|r| r.kind == Kind::Text)
            .filter_map(|r| r.text_color)
            .collect();
        assert!(
            colors.contains(&tokens::MUTED_FOREGROUND),
            "expected comment to map to MUTED_FOREGROUND, got: {colors:?}"
        );
    }
}
